//! Specifies where and how to load the initial config node.

use clap::{Arg, ArgMatches, Command};
use std::fmt;
use std::path::{Path, PathBuf};

use super::error::SetupError;
use super::query_args::QueryArgs;
use crate::node::{default_format_parser_registry, FormatParserRegistry, Node, NodeError};
use crate::rhai::eval_script_var;
use crate::util::PathExt;
use crate::BoxedError;

// ---------------------------------------------------------------------------------------------- //

/// Specifies where and how to load the initial config node.
///
/// This struct configures how a CLI command discovers and loads its config file:
/// - `positional`: Accept config path as a positional argument (e.g., `myapp config.json`)
/// - `option`: Accept config path via a named flag (e.g., `myapp --config config.json`)
/// - `discover`: Function to discover config when not explicitly provided
/// - `path_policy`: How to handle ambiguous or missing config paths
/// - `loader`: Function that parses a file path into a `Node`
///
/// # Example
///
/// ```ignore
/// ConfigSource::new()
///     .positional("CONFIG", "Path to config file", Required::No)
///     .discover(|cmd| discover_config(cmd))
/// ```
pub struct ConfigSource {
    /// Configuration for positional config path argument.
    pub(crate) positional: Option<PositionalConfig>,
    /// Configuration for named option config path argument (e.g., --config).
    pub(crate) option: Option<OptionConfig>,
    /// Discovery function called when no explicit config path is provided.
    pub(crate) discover: Option<Box<DiscoverFn>>,
    /// Policy for resolving config paths across multiple sources and missing paths.
    pub(crate) path_policy: ConfigPathPolicy,
    /// Function that parses a file path into a Node.
    pub(crate) loader: Box<NodeLoaderFn>,
}

impl Default for ConfigSource {
    fn default() -> Self {
        Self {
            positional: None,
            option: None,
            discover: None,
            path_policy: ConfigPathPolicy::default(),
            loader: Box::new(default_loader),
        }
    }
}

// ---------------------------------------------------------------------------------------------- //

/// Configuration for a positional config path argument.
pub(crate) struct PositionalConfig {
    /// The clap argument ID/name for this positional.
    pub name: String,
    /// Whether this argument is required.
    pub required: bool,
    /// Custom help text.
    pub help: Option<String>,
}

// ---------------------------------------------------------------------------------------------- //

/// Configuration for a named option config path argument.
pub(crate) struct OptionConfig {
    /// The long flag name (e.g., "config" for --config).
    pub long: String,
    /// Optional short flag character.
    pub short: Option<char>,
    /// Custom help text.
    pub help: Option<String>,
}

// ---------------------------------------------------------------------------------------------- //

/// Specifies whether an argument is required or optional.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Required {
    /// The argument is required.
    #[default]
    Yes,
    /// The argument is optional.
    No,
}

// ---------------------------------------------------------------------------------------------- //

/// Policy for resolving config path when multiple sources are configured.
///
/// When both positional and option arguments are configured, this determines
/// how to handle the case where both are provided.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MultiSourcePolicy {
    /// Error if multiple sources provide a config path.
    #[default]
    ErrorOnMultiple,
    /// Use the first source that provides a config path (positional > option > discovery).
    PreferPositional,
    /// Use the option if provided, otherwise positional, otherwise discovery.
    PreferOption,
}

// ---------------------------------------------------------------------------------------------- //

/// Policy for handling the case where no config path is resolved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MissingConfigPolicy {
    /// Error when no config path is available.
    #[default]
    Error,
    /// Start from an empty config node instead of a file.
    EmptyNode,
}

// ---------------------------------------------------------------------------------------------- //

/// Policies for resolving config paths across multiple inputs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConfigPathPolicy {
    /// Behavior when multiple sources provide a config path.
    pub multiple: MultiSourcePolicy,
    /// Behavior when no config path can be resolved.
    pub missing: MissingConfigPolicy,
}

/// Function type for auto-discovering config file paths.
pub type DiscoverFn = dyn Fn(&str) -> Result<PathBuf, BoxedError>;

/// Node loader function type.
pub type NodeLoaderFn = dyn Fn(&Path) -> Result<Node, BoxedError>;

// ---------------------------------------------------------------------------------------------- //

/// The resolved config input for a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedConfigInput {
    /// Load the initial config node from this path.
    Path(PathBuf),
    /// Start from an empty config node.
    Empty,
}

// ---------------------------------------------------------------------------------------------- //

impl ConfigSource {
    /// Creates a new config source with default settings.
    ///
    /// No positional or option arguments are configured. The default loader is
    /// `Node::parse_file`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a positional argument for the config path.
    pub fn positional(
        mut self,
        name: impl Into<String>,
        help: impl Into<String>,
        required: Required,
    ) -> Self {
        self.positional = Some(PositionalConfig {
            name: name.into(),
            required: matches!(required, Required::Yes),
            help: Some(help.into()),
        });
        self
    }

    /// Sets a named option argument for the config path (e.g., `--config`).
    pub fn option(
        mut self,
        long: impl Into<String>,
        short: Option<char>,
        help: impl Into<String>,
    ) -> Self {
        self.option = Some(OptionConfig {
            long: long.into(),
            short,
            help: Some(help.into()),
        });
        self
    }

    /// Sets a discovery function for finding config when no explicit path is provided.
    ///
    /// The closure receives the command name and returns the discovered path.
    /// The error type can be any type that converts into a boxed error, including
    /// `anyhow::Error`, `std::io::Error`, or any `E: std::error::Error`.
    pub fn discover<F, E>(mut self, f: F) -> Self
    where
        F: Fn(&str) -> Result<PathBuf, E> + 'static,
        E: Into<BoxedError> + 'static,
    {
        self.discover = Some(Box::new(move |cmd| f(cmd).map_err(|e| e.into())));
        self
    }

    /// Sets both config path resolution policies at once.
    pub fn path_policy(mut self, policy: ConfigPathPolicy) -> Self {
        self.path_policy = policy;
        self
    }

    /// Sets the behavior used when multiple config sources provide a path.
    pub fn multi_source_policy(mut self, policy: MultiSourcePolicy) -> Self {
        self.path_policy.multiple = policy;
        self
    }

    /// Sets the behavior used when no config path can be resolved.
    pub fn missing_policy(mut self, policy: MissingConfigPolicy) -> Self {
        self.path_policy.missing = policy;
        self
    }

    /// Starts from an empty config node when no config path can be resolved.
    pub fn or_empty(self) -> Self {
        self.missing_policy(MissingConfigPolicy::EmptyNode)
    }

    /// Sets the loader function that parses a file path into a Node.
    ///
    /// If not called, defaults to `Node::parse_file`.
    pub fn loader<F, E>(mut self, f: F) -> Self
    where
        F: Fn(&Path) -> Result<Node, E> + 'static,
        E: Into<BoxedError> + 'static,
    {
        self.loader = Box::new(move |p| f(p).map_err(|e| e.into()));
        self
    }

    /// Sets the loader to extract a named variable from a Rhai config file.
    pub fn rhai_var_loader(self, var_name: impl Into<String>) -> Self {
        let var_name = var_name.into();
        self.loader(move |path| load_rhai_var_node(path, &var_name))
    }

    // ---------------------------------------------------------------------------------------------- //

    /// Adds config path arguments to a clap Command (positional and/or flag).
    ///
    /// This adds the configured positional argument (e.g., `CONFIG`) and/or named option
    /// (e.g., `--config`) for specifying the config file path.
    ///
    /// The `query_args` parameter is used to check if `--desc` is available, which allows
    /// the config path to be optional when `--desc` is present.
    #[must_use]
    pub fn augment(&self, mut cmd: Command, query_args: &QueryArgs) -> Command {
        // Add positional config path argument.
        if let Some(pos) = &self.positional {
            let name = pos.name.clone();
            let mut arg = Arg::new(name).value_parser(clap::value_parser!(PathBuf));

            // If --desc is available, make CONFIG not required when --desc is present.
            let desc_arg_name = query_args.desc.as_ref().map(|d| &d.long);
            if pos.required {
                if let Some(desc_name) = desc_arg_name {
                    arg = arg.required_unless_present(desc_name.clone());
                } else {
                    arg = arg.required(true);
                }
            }

            // Add help text (custom or default).
            let help_text = match &pos.help {
                Some(custom) => custom.clone(),
                None => "Path to config file".to_string(),
            };
            arg = arg.help(help_text);

            cmd = cmd.arg(arg);
        }

        // Add --config option if specified.
        if let Some(opt) = &self.option {
            let long = opt.long.clone();
            let mut arg =
                Arg::new(long.clone()).long(long).value_parser(clap::value_parser!(PathBuf));

            if let Some(short) = opt.short {
                arg = arg.short(short);
            }

            // Add help text (custom or default).
            let help_text = match &opt.help {
                Some(custom) => custom.clone(),
                None => "Path to config file".to_string(),
            };
            arg = arg.help(help_text);

            cmd = cmd.arg(arg);
        }

        cmd
    }

    /// Resolves the initial config input from arguments or discovery.
    ///
    /// The resolution order depends on the configured `MultiSourcePolicy`:
    /// - `ErrorOnMultiple`: Error if both positional and option are provided.
    /// - `PreferPositional`: Use positional if provided, otherwise option, otherwise discovery.
    /// - `PreferOption`: Use option if provided, otherwise positional, otherwise discovery.
    pub fn resolve_input(
        &self,
        cmd_name: &str,
        matches: &ArgMatches,
    ) -> Result<ResolvedConfigInput, SetupError> {
        // Get positional path if present.
        let positional_path = match &self.positional {
            Some(pos) => matches.get_one::<PathBuf>(&pos.name).cloned(),
            None => None,
        };
        // Get option path if present.
        let option_path = match &self.option {
            Some(opt) => matches.get_one::<PathBuf>(&opt.long).cloned(),
            None => None,
        };

        // Apply policy.
        let resolved = match self.path_policy.multiple {
            MultiSourcePolicy::ErrorOnMultiple => {
                if positional_path.is_some() && option_path.is_some() {
                    return Err(SetupError::MultipleConfigPaths);
                }
                positional_path.or(option_path)
            }
            MultiSourcePolicy::PreferPositional => positional_path.or(option_path),
            MultiSourcePolicy::PreferOption => option_path.or(positional_path),
        };

        // Return if we have a path from arguments.
        if let Some(path) = resolved {
            return Ok(ResolvedConfigInput::Path(path));
        }

        // Try discovery.
        if let Some(discover) = &self.discover {
            return discover(cmd_name).map(ResolvedConfigInput::Path).map_err(|source| {
                SetupError::ConfigNotFound {
                    cmd_name: cmd_name.to_string(),
                    source,
                }
            });
        }

        match self.path_policy.missing {
            MissingConfigPolicy::Error => Err(SetupError::NoConfigFile {
                cmd_name: cmd_name.to_string(),
            }),
            MissingConfigPolicy::EmptyNode => Ok(ResolvedConfigInput::Empty),
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Node Loader

/// Default loader that uses `Node::parse_file`.
pub fn default_loader(path: &Path) -> Result<Node, BoxedError> {
    Node::parse_file(path).map_err(|e| Box::new(e) as BoxedError)
}

/// Loads a named variable from a Rhai config file as a `Node`.
pub fn load_rhai_var_node(path: impl AsRef<Path>, var_name: &str) -> Result<Node, NodeError> {
    let path = path.as_ref();
    let ext = path
        .ext_str()
        .map_err(|err| NodeError::read_file_extension(path, err))?
        .to_ascii_lowercase();

    if ext != "rhai" {
        return Err(NodeError::new(format!(
            "Rhai variable extraction is only supported for .rhai files, not .{} ({})",
            ext,
            path.display()
        )));
    }

    let dynamic = eval_script_var(path, var_name)?;
    Node::from_rhai_dynamic(dynamic)
}

// ---------------------------------------------------------------------------------------------- //
// Config Discovery Utilities

/// Returns the home config directory: `~/.config/<app_name>/`.
///
/// Returns `None` if the home directory cannot be determined.
pub fn home_config_dir(app_name: &str) -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".config").join(app_name))
}

/// Searches for a config file named `<name>.<ext>` in the given directory.
///
/// Tries each extension in order. If the directory does not exist, returns `NotFound`.
pub fn find_config(
    dir: &Path,
    name: &str,
    extensions: &[&str],
) -> Result<PathBuf, FindConfigError> {
    if !dir.exists() {
        return Err(FindConfigError::NotFound {
            name: name.to_owned(),
            dir: dir.to_owned(),
        });
    }

    let mut matches: Vec<PathBuf> = Vec::new();
    for ext in extensions {
        let candidate = dir.join(format!("{}.{}", name, ext));
        if candidate.is_file() {
            matches.push(candidate);
        }
    }

    match matches.len() {
        0 => Err(FindConfigError::NotFound {
            name: name.to_owned(),
            dir: dir.to_owned(),
        }),
        1 => Ok(matches.remove(0)),
        _ => Err(FindConfigError::Ambiguous {
            name: name.to_owned(),
            dir: dir.to_owned(),
            paths: matches,
        }),
    }
}

/// Searches for a config file named `<name>.<ext>` using extensions from a format registry.
///
/// Tries each registered extension in order. If the directory does not exist, returns `NotFound`.
pub fn find_config_with_registry(
    dir: &Path,
    name: &str,
    registry: &FormatParserRegistry,
) -> Result<PathBuf, FindConfigError> {
    let extensions = registry.supported_extensions();
    find_config(dir, name, &extensions)
}

/// Searches for a config file named `<name>.<ext>` using the default format registry.
pub fn find_config_default(dir: &Path, name: &str) -> Result<PathBuf, FindConfigError> {
    let registry = default_format_parser_registry();
    find_config_with_registry(dir, name, &registry)
}

// ---------------------------------------------------------------------------------------------- //

/// Errors from config file resolution.
#[derive(Debug)]
pub enum FindConfigError {
    /// No matching config file was found.
    NotFound { name: String, dir: PathBuf },
    /// Multiple matching config files were found.
    Ambiguous {
        name: String,
        dir: PathBuf,
        paths: Vec<PathBuf>,
    },
}

impl fmt::Display for FindConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FindConfigError::NotFound { name, dir } => {
                write!(f, "no '{}' config file found in '{}'", name, dir.display())
            }
            FindConfigError::Ambiguous { name, dir, paths } => {
                write!(f, "multiple '{}' config files in '{}':", name, dir.display())?;
                for path in paths {
                    write!(f, "\n  {}", path.display())?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for FindConfigError {}

// ---------------------------------------------------------------------------------------------- //
// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn load_rhai_var_node_extracts_exported_variable() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("finance.rhai");
        fs::write(
            &path,
            r#"
                export let append = #{
                    input: "current.rhai",
                    dry_run: true,
                };
            "#,
        )
        .unwrap();

        let node = load_rhai_var_node(&path, "append").unwrap();

        assert_eq!(node.req::<String>("input").unwrap(), "current.rhai");
        assert!(node.req::<bool>("dry_run").unwrap());
    }

    #[test]
    fn rhai_var_loader_sets_config_source_loader() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("vid.rhai");
        fs::write(
            &path,
            r#"
                export let convert = #{
                    output: "out.webm",
                };
            "#,
        )
        .unwrap();

        let source = ConfigSource::new().rhai_var_loader("convert");
        let node = (source.loader)(&path).unwrap();

        assert_eq!(node.req::<String>("output").unwrap(), "out.webm");
    }

    #[test]
    fn load_rhai_var_node_rejects_non_rhai_files() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("config.json");
        fs::write(&path, r#"{"append": {}}"#).unwrap();

        let err = load_rhai_var_node(&path, "append").unwrap_err();

        assert!(err.to_string().contains("only supported for .rhai files"));
    }
}
