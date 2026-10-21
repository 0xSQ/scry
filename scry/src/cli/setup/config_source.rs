//! Specifies where and how to load the initial config node.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use clap::{Arg, ArgMatches, Command};

use super::error::SetupError;
use super::{ApplicationConfig, CommandConfigSource, ConfigFile};
use crate::node::{default_format_parser_registry, FormatParserRegistry, Node, NodeError};
use crate::rhai::eval_script_var;
use crate::util::PathExt;
use crate::{BoxedError, KeyPath};

// ---------------------------------------------------------------------------------------------- //

/// Specifies where and how to load the initial config node.
///
/// Accepts a config path through a positional argument, a named option, or discovery.
/// Explicit paths take precedence over discovery. Path policies resolve competing explicit
/// sources and missing input. The loader defaults to [`Node::parse_file`].
/// Explicit values accept `FILE::KEYPATH` to select a subtree. Discovered paths are literal paths.
///
/// # Example
///
/// ```
/// use scry::cli::setup::ConfigSource;
///
/// let source = ConfigSource::new().option("config", Some('C'), "Path to config file.");
/// ```
#[derive(Default)]
pub struct ConfigSource {
    /// Configuration for positional config path argument.
    positional: Option<PositionalConfig>,
    /// Configuration for named option config path argument (e.g., --config).
    option: Option<OptionConfig>,
    /// Conventional command discovery, using the complete registered CLI tree.
    application: Option<ApplicationConfig>,
    /// Discovery function called when no explicit config path is provided.
    discover: Option<Box<DiscoverFn>>,
    /// Policy for resolving config paths across multiple sources and missing paths.
    path_policy: ConfigPathPolicy,
    /// Optional replacement for the standard file loader.
    loader: Option<Box<NodeLoaderFn>>,
}

impl ConfigSource {
    /// Creates a new config source with default settings.
    ///
    /// No positional or option arguments are configured. The default loader is
    /// `Node::parse_file`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a positional argument for the config path.
    ///
    /// The parser accepts an omitted path. Source resolution applies the missing-config policy
    /// when the invocation needs input.
    /// An explicit value accepts a whole `FILE` or `FILE::KEYPATH`.
    pub fn positional(mut self, name: impl Into<String>, help: impl Into<String>) -> Self {
        self.positional = Some(PositionalConfig {
            name: name.into(),
            help: help.into(),
        });
        self
    }

    /// Sets a named option argument for the config path (e.g., `--config`).
    ///
    /// An explicit value accepts a whole `FILE` or `FILE::KEYPATH`.
    pub fn option(
        mut self,
        long: impl Into<String>,
        short: Option<char>,
        help: impl Into<String>,
    ) -> Self {
        self.option = Some(OptionConfig {
            long: long.into(),
            short,
            help: help.into(),
        });
        self
    }

    /// Sets a discovery function for finding config when no explicit path is provided.
    ///
    /// The closure receives the command name and returns the discovered path, or `Ok(None)` to
    /// fall through to the missing-config policy (error by default, or an empty node with
    /// [`or_empty`](Self::or_empty)). The error type can be any type that converts into a boxed
    /// error, including `anyhow::Error`, `std::io::Error`, or any `E: std::error::Error`.
    ///
    /// This hook owns its lookup policy, such as finding a project file from the current
    /// directory. It receives a command name rather than the registered command tree. Use
    /// [`application`](Self::application) for conventional nested command discovery instead.
    /// These source modes cannot be combined.
    pub fn discover<F, E>(mut self, f: F) -> Self
    where
        F: Fn(&str) -> Result<Option<PathBuf>, E> + 'static,
        E: Into<BoxedError> + 'static,
    {
        self.discover = Some(Box::new(move |cmd| f(cmd).map_err(|e| e.into())));
        self
    }

    /// Enables conventional discovery using the registered command tree and application name.
    ///
    /// Uses each ancestor command file and the application-wide file under one effective root.
    /// Use [`ApplicationConfig`] to customize the root, formats, or Rhai engine. Custom `discover`
    /// and `loader` closures are separate source modes and cannot be combined with this mode.
    pub fn application(mut self, application: impl Into<ApplicationConfig>) -> Self {
        self.application = Some(application.into());
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
    /// If not called, defaults to [`Node::parse_file`]. The loader receives the decoded file
    /// path. An explicit `::KEYPATH` selection is applied to its result before CLI edits and
    /// typed decoding. Custom loaders cannot be combined with [`application`](Self::application).
    pub fn loader<F, E>(mut self, f: F) -> Self
    where
        F: Fn(&Path) -> Result<Node, E> + 'static,
        E: Into<BoxedError> + 'static,
    {
        self.loader = Some(Box::new(move |p| f(p).map_err(|e| e.into())));
        self
    }

    /// Sets the loader to extract a named variable from a Rhai config file.
    ///
    /// Uses custom loader mode. Conventional application discovery selects returned values and
    /// map members instead of exported variables, so it does not need this loader.
    pub fn rhai_var_loader(self, var_name: impl Into<String>) -> Self {
        let var_name = var_name.into();
        self.loader(move |path| load_rhai_var_node(path, &var_name))
    }

    // ------------------------------------------------------------------------------------------ //

    /// Adds config path arguments to a clap Command (positional and/or flag).
    ///
    /// This adds the configured positional argument (e.g., `CONFIG`) and/or named option
    /// (e.g., `--config`) for specifying the config file path.
    ///
    /// Both arguments are optional in the parser. Source resolution applies the missing-config
    /// policy when the invocation needs input.
    ///
    /// Raw argument values are stored as [`PathBuf`] values containing the original combined
    /// `FILE::KEYPATH` argument. Use [`resolve_input`](Self::resolve_input) or the selected
    /// [`CommandConfigSource`] to obtain the decoded filesystem path.
    #[must_use]
    pub fn augment(&self, mut cmd: Command) -> Command {
        if let Some(pos) = &self.positional {
            cmd = cmd.arg(
                Arg::new(pos.name.clone())
                    .value_parser(clap::value_parser!(PathBuf))
                    .help(pos.help.clone()),
            );
        }

        if let Some(opt) = &self.option {
            let mut arg = Arg::new(opt.long.clone())
                .long(opt.long.clone())
                .value_name("FILE[::KEY]")
                .value_parser(clap::value_parser!(PathBuf))
                .help(opt.help.clone());

            if let Some(short) = opt.short {
                arg = arg.short(short);
            }

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
    ///
    /// Explicit values are parsed after choosing a source according to the policy. The returned
    /// file retains its selection. Custom discovery returns literal paths with an empty selection.
    /// Conventional application discovery needs a command tree and runs through [`super::Setup`]
    /// or [`ApplicationConfig`].
    pub fn resolve_input(
        &self,
        cmd_name: &str,
        matches: &ArgMatches,
    ) -> Result<ResolvedConfigInput, SetupError> {
        if let Some(file) = self.explicit_file(matches)? {
            return Ok(ResolvedConfigInput::File(file));
        }
        if self.application.is_some() {
            return Err(SetupError::ApplicationContextRequired);
        }

        // Try discovery. A `None` result falls through to the missing-config policy.
        if let Some(discover) = &self.discover {
            match discover(cmd_name) {
                Ok(Some(path)) => {
                    return Ok(ResolvedConfigInput::File(ConfigFile {
                        path,
                        selection: KeyPath::new(),
                    }))
                }
                Ok(None) => {}
                Err(source) => {
                    return Err(SetupError::ConfigNotFound {
                        cmd_name: cmd_name.to_string(),
                        source,
                    });
                }
            }
        }

        self.missing_input(cmd_name)
    }

    fn explicit_file(&self, matches: &ArgMatches) -> Result<Option<ConfigFile>, SetupError> {
        let positional_path =
            self.positional.as_ref().and_then(|pos| matches.get_one::<PathBuf>(&pos.name));
        let option_path =
            self.option.as_ref().and_then(|opt| matches.get_one::<PathBuf>(&opt.long));

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

        resolved
            .map(|value| ConfigFile::parse(value.as_os_str()).map_err(SetupError::from))
            .transpose()
    }

    fn missing_input(&self, cmd_name: &str) -> Result<ResolvedConfigInput, SetupError> {
        match self.path_policy.missing {
            MissingConfigPolicy::Error => Err(SetupError::NoConfigFile {
                cmd_name: cmd_name.to_string(),
            }),
            MissingConfigPolicy::EmptyNode => Ok(ResolvedConfigInput::Empty),
        }
    }

    pub(super) fn load_input(
        &self,
        cmd_name: &str,
        matches: &ArgMatches,
        commands: &[&Command],
    ) -> Result<(Node, Option<CommandConfigSource>), SetupError> {
        if let Some(application) = &self.application {
            if self.discover.is_some() || self.loader.is_some() {
                return Err(SetupError::InvalidSourceConfiguration);
            }
            let root = commands.first().expect("dispatch includes the root command");
            let path: Vec<_> =
                commands.iter().skip(1).map(|command| command.get_name().to_owned()).collect();
            let loaded = match self.explicit_file(matches)? {
                Some(file) => Some(application.load_file(file.path, file.selection)?),
                None => application.load_command(root, &path)?,
            };
            return match loaded {
                Some(loaded) => Ok((loaded.node, Some(loaded.source))),
                None if self.path_policy.missing == MissingConfigPolicy::EmptyNode => {
                    Ok((Node::empty_map(), None))
                }
                None => Err(SetupError::NoCommandConfig {
                    application: application.name().into(),
                    command: super::command_config::command_name(root, &path),
                }),
            };
        }
        match self.resolve_input(cmd_name, matches)? {
            ResolvedConfigInput::Empty => Ok((Node::empty_map(), None)),
            ResolvedConfigInput::File(ConfigFile { path, selection }) => {
                let Some(loader) = &self.loader else {
                    let loaded = super::command_config::load_plain_file(path, selection)?;
                    return Ok((loaded.node, Some(loaded.source)));
                };
                let node = loader(&path).map_err(|source| SetupError::LoadConfigFile {
                    path: path.clone(),
                    source,
                })?;
                let mut node = node.req_node(&selection)?.clone();
                node.reanchor(KeyPath::new());
                let metadata = path.extension().and_then(|ext| ext.to_str()).and_then(|ext| {
                    let registry = default_format_parser_registry();
                    let format = registry.parser_for_extension(ext)?.id();
                    Some(CommandConfigSource {
                        logical_path: path.clone(),
                        physical_path: path.canonicalize_path().ok()?,
                        selection,
                        format,
                    })
                });
                Ok((node, metadata))
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------- //

/// Configuration for a positional config path argument.
struct PositionalConfig {
    /// The clap argument ID/name for this positional.
    name: String,
    /// Custom help text.
    help: String,
}

/// Configuration for a named option config path argument.
struct OptionConfig {
    /// The long flag name (e.g., "config" for --config).
    long: String,
    /// Optional short flag character.
    short: Option<char>,
    /// Custom help text.
    help: String,
}

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

/// Policy for handling the case where no config path is resolved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MissingConfigPolicy {
    /// Error when no config path is available.
    #[default]
    Error,
    /// Start from an empty config node instead of a file.
    EmptyNode,
}

/// Policies for resolving config paths across multiple inputs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConfigPathPolicy {
    /// Behavior when multiple sources provide a config path.
    pub multiple: MultiSourcePolicy,
    /// Behavior when no config path can be resolved.
    pub missing: MissingConfigPolicy,
}

/// Function type for auto-discovering config file paths.
///
/// Returns `Ok(None)` when nothing was found and resolution should fall through to the
/// missing-config policy. Errors abort resolution.
pub type DiscoverFn = dyn Fn(&str) -> Result<Option<PathBuf>, BoxedError>;

/// Node loader function type.
pub type NodeLoaderFn = dyn Fn(&Path) -> Result<Node, BoxedError>;

/// The resolved config input for a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedConfigInput {
    /// Loads the initial config node from this file and selects its payload.
    File(ConfigFile),
    /// Start from an empty config node.
    Empty,
}

// ---------------------------------------------------------------------------------------------- //

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
/// Reports ambiguity when more than one extension matches. If none match, returns `NotFound`.
pub fn find_config(
    dir: &Path,
    name: &str,
    extensions: &[&str],
) -> Result<PathBuf, FindConfigError> {
    let mut matches: Vec<PathBuf> = Vec::new();
    let entries = read_config_directory(dir)
        .map_err(|source| FindConfigError::Io {
            path: dir.to_path_buf(),
            source,
        })?
        .unwrap_or_default();
    for ext in extensions {
        for candidate in entries.iter().filter(|path| {
            path.file_stem().is_some_and(|stem| stem == name)
                && path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case(ext))
        }) {
            let metadata = fs::metadata(candidate).map_err(|source| FindConfigError::Io {
                path: candidate.clone(),
                source,
            })?;
            if metadata.is_file() {
                matches.push(candidate.clone());
            }
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

/// Reads a config directory, allowing absence but preserving invalid paths and broken links.
pub(super) fn read_config_directory(path: &Path) -> io::Result<Option<Vec<PathBuf>>> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            for ancestor in path.ancestors().filter(|path| !path.as_os_str().is_empty()) {
                match fs::symlink_metadata(ancestor) {
                    Ok(_) => {
                        fs::read_dir(ancestor)?;
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let mut paths =
        entries.map(|entry| entry.map(|entry| entry.path())).collect::<io::Result<Vec<_>>>()?;
    paths.sort();
    Ok(Some(paths))
}

/// Searches for a config file named `<name>.<ext>` using extensions from a format registry.
///
/// Reports ambiguity when more than one extension matches. If none match, returns `NotFound`.
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
    /// A candidate could not be inspected, including a broken filesystem link.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
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
            FindConfigError::Io { path, source } => {
                write!(f, "cannot inspect config file '{}': {source}", path.display())
            }
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

impl std::error::Error for FindConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn discovery_requires_one_matching_file_and_reports_all_ambiguous_paths() {
        let temp = TempDir::new().unwrap();
        let directory = temp.path().join("configs");
        let extensions = ["json", "rhai"];
        assert!(matches!(
            find_config(&directory, "app", &extensions),
            Err(FindConfigError::NotFound { .. })
        ));

        fs::create_dir(&directory).unwrap();
        let json = directory.join("app.json");
        let rhai = directory.join("app.rhai");
        fs::create_dir(&json).unwrap();
        fs::write(&rhai, "#{}").unwrap();
        assert_eq!(find_config(&directory, "app", &extensions).unwrap(), rhai);

        fs::remove_dir(&json).unwrap();
        fs::write(&json, "{}").unwrap();
        let error = find_config(&directory, "app", &extensions).unwrap_err();
        let FindConfigError::Ambiguous { name, dir, paths } = error else {
            panic!("both matching files must be reported as ambiguous");
        };
        assert_eq!(name, "app");
        assert_eq!(dir, directory);
        assert_eq!(paths, [json, rhai]);
    }

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
        let node = source.loader.as_ref().unwrap()(&path).unwrap();

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
