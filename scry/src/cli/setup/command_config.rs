//! Conventional command configuration discovery and loading.

use std::collections::{hash_map::Entry, HashMap};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use clap::Command;
use rhai::{Dynamic, Engine};

use super::config_source::{
    find_config_with_registry, home_config_dir, read_config_directory, FindConfigError,
};
use super::rhai_env::load_rhai_env_in;
use crate::node::{default_format_parser_registry, Format, FormatParserRegistry, Kind};
use crate::rhai::RhaiEnv;
use crate::util::{PathError, PathExt};
use crate::{BoxedError, KeyPath, Node};

// ---------------------------------------------------------------------------------------------- //

/// Shared application conventions for command files, namespaces, and the Rhai environment.
///
/// Command identity comes from the application's registered clap command hierarchy.
/// [`crate::cli::Bundle`] forwards borrowed clap commands while dispatching. Clones share the
/// parser registry and engine configuration. CLI resolution happens only when a command needs input.
///
/// # Discovery and namespaces
///
/// A command such as `archive create` can use `archive/create.*`, the `create` member of
/// `archive.*`, or the `archive.create` member of the application-wide `<name>.*`. Paths and map
/// keys retain canonical CLI spelling. Every applicable ancestor is considered. Multiple
/// definitions or supported extensions for a candidate stem are errors, without precedence or
/// merging. A missing member does not claim the command.
///
/// The clap hierarchy distinguishes namespaces from payloads. Leaf payloads use the whole value.
/// A command with children uses a map keyed by child names, with `"$self"` for its own payload.
/// [`Command::is_subcommand_required_set`] determines whether a command can have its own payload.
/// A required subcommand makes it a pure group, where `$self` is invalid. Otherwise it may have
/// its own payload. The application root follows the same rules.
///
/// [`crate::cli::Bundle`] sets subcommand requiredness according to the presence of a handler or
/// fallback. Custom clap trees follow their declared requiredness, without handler verification.
/// Unknown namespace keys are rejected, including keys in sibling namespaces of a consulted
/// ancestor. The child command name `$self` is reserved. Ordinary payload fields have no
/// namespace meaning.
///
/// The root-level stems `root`, the configured environment filename, and the application name
/// are reserved, ignoring ASCII case. Commands with those names use the application-wide map or
/// descendant files instead of a same-stem root file.
///
/// # Rhai evaluation
///
/// Discovery evaluates candidate scripts normally and validates their returned namespace maps.
/// Variables, functions, imports, mutation, conditionals, and early returns are supported when
/// the returned value matches the registered command tree. Exported variables are not discovery
/// entry points.
///
/// Each candidate physical path and format pair is loaded directly once per command load.
/// Imports follow normal Rhai evaluation and can execute a script again. Non-owning ancestors
/// execute too, including their imports and other work. A runtime error or invalid
/// namespace in any consulted ancestor remains an error even when a separate file supplies the
/// payload.
/// Physical source paths anchor relative imports, and separate files retain independent scopes.
///
/// The optional environment is loaded lazily, once for all Rhai candidates. All-non-Rhai loads
/// do not load it. Only the selected Rhai payload is converted to a [`Node`], so unselected
/// payloads may retain raw function pointers or custom values.
///
/// # Settings validation
///
/// Names and environment settings are validated by every loading method, even for explicit
/// non-Rhai files or missing optional configurations. Builders defer validation until loading.
/// [`Setup`](super::Setup) actions that do not load input, such as descriptions and templates,
/// do not trigger this validation.
#[derive(Clone)]
pub struct ApplicationConfig {
    name: String,
    directory: Option<PathBuf>,
    environment_file: String,
    environment_binding: String,
    registry: Rc<FormatParserRegistry>,
    configure_engine: Rc<ConfigureEngine>,
}

impl ApplicationConfig {
    /// Creates conventions rooted at `~/.config/<name>/`, including on Windows.
    ///
    /// Uses the default parser registry and the optional `env.*` environment bound as `env`.
    /// The application name determines the directory and application-wide filename, independently
    /// of the executable name. It must be one path component other than `root`, and must differ
    /// from the configured environment filename stem, ignoring ASCII case.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            directory: None,
            environment_file: "env".into(),
            environment_binding: "env".into(),
            registry: Rc::new(default_format_parser_registry()),
            configure_engine: Rc::new(|_, _| {}),
        }
    }

    /// Sets the initial directory, before resolving an optional `root.*` pointer.
    ///
    /// Replaces the conventional `~/.config/<name>/` location. An optional `root.*` pointer selects
    /// the single directory used for command and environment discovery. No local overlay or
    /// fallback to the initial directory is applied after a pointer selects another root.
    pub fn directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.directory = Some(directory.into());
        self
    }

    /// Sets the filename stem used for the optional application environment.
    ///
    /// Defaults to `env`. The active registry selects the format, so `machine` discovers
    /// `machine.rhai`, `machine.json`, or another enabled extension. The stem is reserved for
    /// this bootstrap role. Renaming it frees `env.*` for ordinary command configuration.
    /// It must be a portable filename stem without a path or extension, distinct from `root`
    /// and the application name regardless of ASCII case. Validation happens when configuration
    /// is loaded.
    pub fn environment_file(mut self, stem: impl Into<String>) -> Self {
        self.environment_file = stem.into();
        self
    }

    /// Sets the Rhai constant name used for the loaded application environment.
    ///
    /// Defaults to `env`. The binding is injected into command scripts and imported modules.
    /// Its name does not change file discovery or reserve command filenames. It must be a
    /// nonreserved Rhai variable identifier. Validation happens when configuration is loaded.
    /// Changing the binding does not change which value the environment script returns.
    pub fn environment_binding(mut self, binding: impl Into<String>) -> Self {
        self.environment_binding = binding.into();
        self
    }

    /// Sets the enabled input formats, including the root pointer and environment file.
    ///
    /// Replaces the default registry. The logical filename's extension selects the parser before
    /// following filesystem links. Multiple supported extensions for one candidate stem remain
    /// ambiguous.
    pub fn registry(mut self, registry: FormatParserRegistry) -> Self {
        self.registry = Rc::new(registry);
        self
    }

    /// Configures Rhai candidate and explicit-file evaluation.
    ///
    /// The callback receives the loaded environment and an engine with its relative module
    /// resolver installed. The environment's constant is injected into evaluation scopes for
    /// each candidate and recursively imported modules. One engine is configured per command
    /// load and reused for all Rhai candidates, including non-owning ancestors.
    /// Root pointers and environment bootstrap files never receive this configuration.
    pub fn rhai_engine(mut self, configure: impl Fn(&mut Engine, &RhaiEnv) + 'static) -> Self {
        self.configure_engine = Rc::new(configure);
        self
    }

    /// Returns the application name used for its application-wide config file.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Loads the configured optional environment using the effective application root.
    ///
    /// Searches the configured filename stem using the active registry. Missing files produce
    /// an environment without the configured binding. Competing extensions and invalid files
    /// are errors. A root pointer replaces the environment location along with the command root.
    ///
    /// Rhai supplies its final returned value, retaining raw values such as function pointers.
    /// An exported variable is used only if explicitly returned. Other formats supply their root
    /// map. Bootstrap evaluation uses a fresh engine with relative imports, without the command
    /// engine's functions or injected constants. Environments are not merged.
    ///
    /// Custom direct loaders can install the returned [`RhaiEnv::module_resolver`] on their
    /// engine and evaluate files through [`RhaiEnv::eval`].
    pub fn load_environment(&self) -> Result<RhaiEnv, CommandConfigError> {
        self.validate_settings()?;
        let root = self.resolve_root()?;
        self.environment_in(&root)
    }

    /// Loads the requested command and preserves its discovered source and selection.
    ///
    /// Receives the complete registered clap root and a canonical command path excluding the
    /// root and invocation name. An empty path selects the application root. Direct callers
    /// resolve aliases before supplying the path. [`crate::cli::Bundle`] provides the borrowed
    /// command metadata automatically during dispatch.
    ///
    /// Own-payload support follows clap's declared subcommand requiredness, as described on
    /// [`ApplicationConfig`].
    ///
    /// Discovery follows the namespace and ambiguity rules on [`ApplicationConfig`]. Invalid
    /// consulted ancestors remain errors even when a separate file defines the requested command.
    ///
    /// Returns `Ok(None)` only when no definition exists. The caller chooses whether absence
    /// is required or optional. Selected input is reanchored at an empty [`KeyPath`], before
    /// typed decoding or CLI edits. Direct candidate loads reuse results by physical path and
    /// format. Rhai candidates share one lazily loaded environment, including non-owning ancestors.
    /// With no Rhai candidates, the environment is not loaded.
    pub fn load_command(
        &self,
        root: &Command,
        path: &[String],
    ) -> Result<Option<LoadedCommandConfig>, CommandConfigError> {
        self.validate(root, path)?;
        let config_root = self.resolve_root()?;
        let mut definitions = Vec::new();
        let mut values = HashMap::new();
        let mut runtime = None;
        for length in (0..=path.len()).rev() {
            // Root-level stems with bootstrap or application-wide roles are not command files.
            if length == 1 && self.reserved_stem(&path[0]) {
                continue;
            }
            let (stem, parents) = path[..length].split_last().unwrap_or((&self.name, &[]));
            let directory =
                parents.iter().fold(config_root.clone(), |directory, name| directory.join(name));
            let file = match find_config_with_registry(&directory, stem, &self.registry) {
                Ok(path) => path,
                Err(FindConfigError::NotFound { .. }) => continue,
                Err(error) => return Err(error.into()),
            };
            let mut source = file_source(file, KeyPath::new(), &self.registry)?;
            let command = command_at(root, &path[..length]).expect("validated command tree");
            let key = (source.physical_path.clone(), source.format.clone());
            let value = match values.entry(key) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let value = if source.format == Format::Rhai {
                        if runtime.is_none() {
                            let environment = self.environment_in(&config_root)?;
                            let engine = self.engine(&environment);
                            runtime = Some((environment, engine));
                        }
                        let (environment, engine) = runtime.as_ref().unwrap();
                        ConfigValue::Rhai(eval_rhai(&source, environment, engine)?)
                    } else {
                        ConfigValue::Node(self.parse(&source)?)
                    };
                    entry.insert(value)
                }
            };
            if let Some(selection) = namespace_selection(
                &value.namespace_shape(command),
                command,
                &source.logical_path,
                &KeyPath::from_keys(&path[length..]),
                &KeyPath::new(),
            )? {
                source.selection = selection;
                definitions.push(source);
            }
        }
        if definitions.len() > 1 {
            return Err(CommandConfigError::Ambiguous {
                command: command_name(root, path),
                sources: definitions,
            });
        }
        let Some(source) = definitions.pop() else {
            return Ok(None);
        };
        let value = values
            .remove(&(source.physical_path.clone(), source.format.clone()))
            .expect("a discovered definition has a loaded value");
        let mut node = value.into_node(&source)?;
        node.reanchor(KeyPath::new());
        Ok(Some(LoadedCommandConfig { node, source }))
    }

    /// Loads an explicit whole payload, or an explicitly selected subtree.
    ///
    /// An empty selection loads the whole returned value or document. The filename and command
    /// tree do not imply a selector. Explicit input bypasses conventional discovery, namespace
    /// validation, and duplicate checks. The selected node is reanchored at an empty [`KeyPath`].
    ///
    /// An explicit non-Rhai file does not consult the ambient root or environment. Rhai input
    /// resolves the root once to obtain its configured optional environment and uses the custom
    /// command engine. Application settings are validated in either case.
    ///
    /// This method accepts an already decoded filename and selection. Use
    /// [`ConfigFile::parse`](super::ConfigFile::parse) to parse the CLI's combined `FILE::KEYPATH`
    /// syntax without filesystem access.
    pub fn load_file(
        &self,
        path: impl Into<PathBuf>,
        selection: KeyPath,
    ) -> Result<LoadedCommandConfig, CommandConfigError> {
        self.validate_settings()?;
        let source = file_source(path.into(), selection, &self.registry)?;
        let mut node = if source.format == Format::Rhai {
            let root = self.resolve_root()?;
            self.eval(&source, &root)?
        } else {
            self.parse(&source)?.req_node(&source.selection)?.clone()
        };
        node.reanchor(KeyPath::new());
        Ok(LoadedCommandConfig { node, source })
    }

    fn resolve_root(&self) -> Result<PathBuf, CommandConfigError> {
        let directory = self
            .directory
            .clone()
            .or_else(|| home_config_dir(&self.name))
            .ok_or(CommandConfigError::NoHomeDir)?;
        resolve_root_directory(&directory, &self.registry).map_err(|source| {
            CommandConfigError::Load {
                path: directory,
                source,
            }
        })
    }

    fn eval(&self, source: &CommandConfigSource, root: &Path) -> Result<Node, CommandConfigError> {
        let env = self.environment_in(root)?;
        let engine = self.engine(&env);
        ConfigValue::Rhai(eval_rhai(source, &env, &engine)?).into_node(source)
    }

    fn environment_in(&self, root: &Path) -> Result<RhaiEnv, CommandConfigError> {
        load_rhai_env_in(root, &self.registry, &self.environment_file, &self.environment_binding)
            .map_err(|error| CommandConfigError::Load {
                path: root.join(format!("{}.*", self.environment_file)),
                source: error,
            })
    }

    fn engine(&self, env: &RhaiEnv) -> Engine {
        let mut engine = Engine::new();
        engine.set_module_resolver(env.module_resolver());
        (self.configure_engine)(&mut engine, env);
        engine
    }

    fn parse(&self, source: &CommandConfigSource) -> Result<Node, CommandConfigError> {
        self.registry
            .parser_by_id(&source.format)
            .expect("resolved parser")
            .parse_file(&source.physical_path)
            .map_err(|error| CommandConfigError::Load {
                path: source.logical_path.clone(),
                source: Box::new(error),
            })
    }

    fn reserved_stem(&self, name: &str) -> bool {
        ["root", &self.environment_file, &self.name]
            .iter()
            .any(|stem| name.eq_ignore_ascii_case(stem))
    }

    fn validate(&self, root: &Command, path: &[String]) -> Result<(), CommandConfigError> {
        self.validate_settings()?;
        validate_tree(root)?;
        if command_at(root, path).is_none() {
            return Err(CommandConfigError::InvalidCommandPath {
                path: path.to_vec(),
            });
        }
        Ok(())
    }

    fn validate_settings(&self) -> Result<(), CommandConfigError> {
        if !valid_component(&self.name) || self.name.eq_ignore_ascii_case("root") {
            return Err(CommandConfigError::InvalidApplicationName {
                name: self.name.clone(),
            });
        }
        if !valid_environment_stem(&self.environment_file) {
            return Err(CommandConfigError::InvalidEnvironmentFile {
                stem: self.environment_file.clone(),
                message: "expected a portable filename stem without a path or extension".into(),
            });
        }
        if self.environment_file.eq_ignore_ascii_case("root")
            || self.environment_file.eq_ignore_ascii_case(&self.name)
        {
            return Err(CommandConfigError::InvalidEnvironmentFile {
                stem: self.environment_file.clone(),
                message: "the environment filename must differ from 'root' and the application-wide filename".into(),
            });
        }
        if !valid_environment_binding(&self.environment_binding) {
            return Err(CommandConfigError::InvalidEnvironmentBinding {
                binding: self.environment_binding.clone(),
            });
        }
        Ok(())
    }
}

/// Returns the supplied directory or the destination of its sole `root.*` file.
///
/// The pointer contains only `directory`. Relative paths use the pointer's physical parent,
/// `~/` expands the home directory, and the destination cannot contain another pointer.
fn resolve_root_directory(
    directory: &Path,
    registry: &FormatParserRegistry,
) -> Result<PathBuf, BoxedError> {
    let pointer = match find_config_with_registry(directory, "root", registry) {
        Ok(path) => path,
        Err(FindConfigError::NotFound { .. }) => return Ok(directory.to_path_buf()),
        Err(error) => return Err(error.into()),
    };
    if read_config_directory(directory)?.unwrap_or_default().len() != 1 {
        return Err(format!(
            "root pointer '{}' must be the only directory entry",
            pointer.display()
        )
        .into());
    }

    let node = Node::parse_file_with_registry(&pointer, registry)?;
    node.ensure_only_keys(&["directory"])?;
    let target = node.req::<String>("directory")?;
    if target.is_empty() {
        return Err("root pointer directory must not be empty".into());
    }
    let pointer = pointer.canonicalize_path()?;
    let target =
        if let Some(suffix) = target.strip_prefix("~/").or_else(|| target.strip_prefix("~\\")) {
            dirs::home_dir().ok_or("cannot determine the home directory")?.join(suffix)
        } else {
            pointer.parent().expect("canonical files have a parent").join(target)
        };
    target.require_dir()?;
    match find_config_with_registry(&target, "root", registry) {
        Err(FindConfigError::NotFound { .. }) => Ok(target),
        Ok(_) | Err(FindConfigError::Ambiguous { .. }) => {
            Err("root pointers cannot be chained".into())
        }
        Err(error) => Err(error.into()),
    }
}

impl From<&str> for ApplicationConfig {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ApplicationConfig {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// A loaded command payload, anchored at the empty key path.
///
/// Parsing and value selection are complete. Typed decoding and CLI edits have not been applied.
/// The source retains the original selection even though the node's paths are payload-relative.
#[derive(Debug)]
pub struct LoadedCommandConfig {
    /// Selected settings after anchoring at the payload root.
    pub node: Node,
    /// File identity and selection before anchoring or CLI edits.
    pub source: CommandConfigSource,
}

/// The logical placement, physical file, and selected subtree of a command configuration.
///
/// Describes the original source before payload reanchoring or CLI edits. Both conventional and
/// explicit loading preserve this metadata. [`Setup::into_bundle_with_source`](super::Setup::into_bundle_with_source)
/// exposes it to CLI payload handlers. An empty-map fallback has no file source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandConfigSource {
    /// Discovered placement or decoded explicit filename, retaining filesystem links.
    pub logical_path: PathBuf,
    /// Canonical file used for evaluation and relative imports.
    pub physical_path: PathBuf,
    /// Subtree within the original returned value or document, empty for the whole payload.
    pub selection: KeyPath,
    /// Format selected by the logical filename, before following links.
    pub format: Format,
}

/// Errors from conventional command configuration discovery.
///
/// Missing conventional definitions use `Ok(None)` from [`ApplicationConfig::load_command`].
/// Invalid settings, consulted namespaces, selected values, and explicit files are errors.
#[derive(Debug, thiserror::Error)]
pub enum CommandConfigError {
    #[error("cannot determine the home directory")]
    NoHomeDir,
    #[error(
        "invalid application config name '{name}': expected one path component other than 'root'"
    )]
    InvalidApplicationName { name: String },
    #[error("invalid environment filename stem '{stem}': {message}")]
    InvalidEnvironmentFile { stem: String, message: String },
    #[error(
        "invalid environment binding '{binding}': expected a nonreserved Rhai variable identifier"
    )]
    InvalidEnvironmentBinding { binding: String },
    #[error("invalid or unregistered command path: {path:?}")]
    InvalidCommandPath { path: Vec<String> },
    #[error("invalid command tree at '{command}': {message}")]
    InvalidCommandTree { command: String, message: String },
    #[error(transparent)]
    Find(#[from] FindConfigError),
    #[error(transparent)]
    Node(#[from] crate::NodeError),
    #[error(transparent)]
    Format(#[from] crate::node::FormatError),
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("failed to load config file '{}'", path.display())]
    Load {
        path: PathBuf,
        #[source]
        source: BoxedError,
    },
    /// Several files or namespace entries define the requested command.
    #[error("multiple definitions for command '{command}':{}", format_sources(.sources))]
    Ambiguous {
        command: String,
        sources: Vec<CommandConfigSource>,
    },
    /// A consulted namespace has an invalid shape, unknown key, or disallowed `$self` entry.
    #[error("invalid command namespace in '{}' at '{selection}': {message}", path.display())]
    Namespace {
        path: PathBuf,
        selection: KeyPath,
        message: String,
    },
}

type ConfigureEngine = dyn Fn(&mut Engine, &RhaiEnv);

enum ConfigValue {
    Rhai(Dynamic),
    Node(Node),
}

impl ConfigValue {
    fn namespace_shape(&self, command: &Command) -> NamespaceShape {
        match self {
            Self::Rhai(value) => dynamic_shape(value, command),
            Self::Node(node) => node_shape(node, command),
        }
    }

    fn into_node(self, source: &CommandConfigSource) -> Result<Node, CommandConfigError> {
        match self {
            Self::Rhai(value) => {
                let selected = select_dynamic(value, &source.selection)?;
                Node::from_rhai_dynamic(selected).map_err(|error| CommandConfigError::Load {
                    path: source.logical_path.clone(),
                    source: Box::new(error),
                })
            }
            Self::Node(node) => Ok(node.req_node(&source.selection)?.clone()),
        }
    }
}

enum NamespaceShape {
    Map(Vec<(String, Self)>),
    Opaque,
}

pub(super) fn command_name(root: &Command, path: &[String]) -> String {
    std::iter::once(root.get_name())
        .chain(path.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_sources(sources: &[CommandConfigSource]) -> String {
    sources
        .iter()
        .map(|source| {
            if source.selection.is_empty() {
                format!("\n  {} (whole value)", source.logical_path.display())
            } else {
                format!("\n  {} at {}", source.logical_path.display(), source.selection)
            }
        })
        .collect()
}

fn valid_component(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && matches!(
            Path::new(name).components().collect::<Vec<_>>().as_slice(),
            [Component::Normal(_)]
        )
}

fn valid_environment_stem(stem: &str) -> bool {
    if !valid_component(stem)
        || stem.contains('.')
        || stem.ends_with(' ')
        || stem.chars().any(|ch| ch.is_control() || matches!(ch, '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return false;
    }
    let upper = stem.to_ascii_uppercase();
    !matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !upper.strip_prefix("COM").or_else(|| upper.strip_prefix("LPT")).is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
        })
}

fn valid_environment_binding(binding: &str) -> bool {
    // The character filter keeps the input to a single name. The compiler then checks Rhai's
    // keyword and variable-name rules without evaluating code.
    binding.chars().all(|ch| ch == '_' || ch.is_alphanumeric())
        && Engine::new().compile(format!("const {binding} = ();")).is_ok()
}

fn command_at<'a>(root: &'a Command, path: &[String]) -> Option<&'a Command> {
    let mut command = root;
    for name in path {
        command = command.get_subcommands().find(|child| child.get_name() == name)?;
    }
    Some(command)
}

fn validate_tree(command: &Command) -> Result<(), CommandConfigError> {
    let mut names = std::collections::HashSet::new();
    for child in command.get_subcommands() {
        if !valid_component(child.get_name()) || child.get_name() == "$self" {
            return Err(CommandConfigError::InvalidCommandTree {
                command: command.get_name().to_owned(),
                message: format!("invalid or reserved child command name '{}'", child.get_name()),
            });
        }
        if !names.insert(child.get_name()) {
            return Err(CommandConfigError::InvalidCommandTree {
                command: command.get_name().to_owned(),
                message: format!("duplicate child command name '{}'", child.get_name()),
            });
        }
        validate_tree(child)?;
    }
    Ok(())
}

pub(super) fn load_plain_file(
    path: PathBuf,
    selection: KeyPath,
) -> Result<LoadedCommandConfig, CommandConfigError> {
    let registry = default_format_parser_registry();
    let source = file_source(path, selection, &registry)?;
    let mut node = if source.format == Format::Rhai {
        let env = RhaiEnv::new();
        let mut engine = Engine::new();
        engine.set_module_resolver(env.module_resolver());
        ConfigValue::Rhai(eval_rhai(&source, &env, &engine)?).into_node(&source)?
    } else {
        registry
            .parser_by_id(&source.format)
            .expect("resolved parser")
            .parse_file(&source.physical_path)?
            .req_node(&source.selection)?
            .clone()
    };
    node.reanchor(KeyPath::new());
    Ok(LoadedCommandConfig { node, source })
}

fn file_source(
    path: PathBuf,
    selection: KeyPath,
    registry: &FormatParserRegistry,
) -> Result<CommandConfigSource, CommandConfigError> {
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    let parser = registry.parser_for_extension(extension).ok_or_else(|| {
        crate::node::FormatError::unknown_file_extension(
            &path,
            extension,
            &registry.supported_extensions(),
        )
    })?;
    let physical_path = path.canonicalize_path()?;
    Ok(CommandConfigSource {
        logical_path: path,
        physical_path,
        selection,
        format: parser.id(),
    })
}

fn node_shape(node: &Node, command: &Command) -> NamespaceShape {
    if !command.has_subcommands() {
        return NamespaceShape::Opaque;
    }
    match &node.kind {
        Kind::Map(map) => NamespaceShape::Map(
            map.iter()
                .map(|(key, value)| {
                    let shape = command
                        .get_subcommands()
                        .find(|child| child.get_name() == key)
                        .map_or(NamespaceShape::Opaque, |child| node_shape(value, child));
                    (key.clone(), shape)
                })
                .collect(),
        ),
        _ => NamespaceShape::Opaque,
    }
}

fn dynamic_shape(value: &Dynamic, command: &Command) -> NamespaceShape {
    if !command.has_subcommands() {
        return NamespaceShape::Opaque;
    }
    let Some(map) = value.read_lock::<rhai::Map>() else {
        return NamespaceShape::Opaque;
    };
    NamespaceShape::Map(
        map.iter()
            .map(|(key, value)| {
                let shape = command
                    .get_subcommands()
                    .find(|child| child.get_name() == key.as_str())
                    .map_or(NamespaceShape::Opaque, |child| dynamic_shape(value, child));
                (key.to_string(), shape)
            })
            .collect(),
    )
}

fn eval_rhai(
    source: &CommandConfigSource,
    environment: &RhaiEnv,
    engine: &Engine,
) -> Result<Dynamic, CommandConfigError> {
    environment.eval(engine, &source.physical_path).map_err(|error| CommandConfigError::Load {
        path: source.logical_path.clone(),
        source: Box::new(error),
    })
}

fn select_dynamic(
    mut value: rhai::Dynamic,
    selection: &KeyPath,
) -> Result<rhai::Dynamic, crate::NodeError> {
    let mut prefix = KeyPath::new();
    for segment in selection.iter() {
        value = match segment {
            crate::key_path::Segment::Key(key) => {
                let map = value.try_cast::<rhai::Map>().ok_or_else(|| {
                    crate::NodeError::invalid_value(
                        &prefix,
                        "expected a map while selecting config payload",
                    )
                })?;
                map.get(key.as_str()).cloned().ok_or_else(|| {
                    crate::NodeError::invalid_value(
                        selection,
                        format!("missing config key '{key}'"),
                    )
                })?
            }
            crate::key_path::Segment::Index(index) => {
                let array = value.try_cast::<rhai::Array>().ok_or_else(|| {
                    crate::NodeError::invalid_value(
                        &prefix,
                        "expected an array while selecting config payload",
                    )
                })?;
                array.get(*index).cloned().ok_or_else(|| {
                    crate::NodeError::invalid_value(
                        selection,
                        format!("missing config index {index}"),
                    )
                })?
            }
        };
        prefix = match segment {
            crate::key_path::Segment::Key(key) => prefix.push_key(key),
            crate::key_path::Segment::Index(index) => prefix.push_index(*index),
        };
    }
    Ok(value)
}

fn namespace_selection(
    shape: &NamespaceShape,
    command: &Command,
    path: &Path,
    requested: &KeyPath,
    selection: &KeyPath,
) -> Result<Option<KeyPath>, CommandConfigError> {
    if !command.has_subcommands() {
        return Ok((selection == requested && !command.is_subcommand_required_set())
            .then(|| selection.clone()));
    }
    let NamespaceShape::Map(entries) = shape else {
        return Err(CommandConfigError::Namespace {
            path: path.to_path_buf(),
            selection: selection.clone(),
            message: "expected a map of child command names".into(),
        });
    };
    let mut payload = None;
    for (key, value) in entries {
        let child_selection = selection.push_key(key);
        if key == "$self" && !command.is_subcommand_required_set() {
            if selection == requested {
                payload = Some(child_selection);
            }
            continue;
        }
        let Some(child) = command.get_subcommands().find(|child| child.get_name() == key) else {
            return Err(CommandConfigError::Namespace {
                path: path.to_path_buf(),
                selection: child_selection,
                message: if key == "$self" {
                    "a pure command group cannot define '$self'".into()
                } else {
                    format!("unknown child command '{key}'")
                },
            });
        };
        if let Some(selected) =
            namespace_selection(value, child, path, requested, &child_selection)?
        {
            payload = Some(selected);
        }
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use crate::node::default_format_parser_registry;
    use crate::util::PathExt;
    use tempfile::tempdir;

    // ---------------------------------------------------------------------------------------------- //

    #[test]
    fn missing_and_ordinary_directories_keep_their_location() {
        let temporary = tempdir().unwrap();
        let registry = default_format_parser_registry();
        let missing = temporary.path().join("missing/nested");
        assert_eq!(resolve_root_directory(&missing, &registry).unwrap(), missing);
        fs::write(temporary.path().join("atlas.rhai"), "throw \"not a pointer\";").unwrap();
        assert_eq!(resolve_root_directory(temporary.path(), &registry).unwrap(), temporary.path());
        assert!(resolve_root_directory(&temporary.path().join("atlas.rhai"), &registry).is_err());
    }

    #[test]
    fn supported_pointer_formats_replace_the_directory() {
        let temporary = tempdir().unwrap();
        let target = temporary.path().join("target");
        fs::create_dir(&target).unwrap();
        for (extension, source) in [
            ("rhai", "let name = \"target\"; #{ directory: \"../\" + name }"),
            #[cfg(feature = "format-json")]
            ("json", r#"{ "directory": "../target" }"#),
            #[cfg(feature = "format-json5")]
            ("json5", "{ directory: '../target' }"),
            #[cfg(feature = "format-toml")]
            ("toml", "directory = '../target'"),
            #[cfg(feature = "format-yaml")]
            ("yaml", "directory: ../target"),
        ] {
            let directory = temporary.path().join(extension);
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join(format!("root.{extension}")), source).unwrap();
            let selected =
                resolve_root_directory(&directory, &default_format_parser_registry()).unwrap();
            assert_eq!(selected.canonicalize_path().unwrap(), target.canonicalize_path().unwrap());
        }
    }

    #[test]
    fn pointers_require_a_valid_target_a_sole_entry_and_one_hop() {
        let temporary = tempdir().unwrap();
        let directory = temporary.path().join("config");
        let target = temporary.path().join("target");
        fs::create_dir(&directory).unwrap();
        fs::create_dir(&target).unwrap();
        let pointer = directory.join("root.rhai");
        let registry = default_format_parser_registry();
        for source in [
            "invalid syntax !",
            "export let config = #{ directory: \"../target\" };",
            "#{ directory: \"\" }",
            "#{ directory: 3 }",
            "#{ directory: \"../target\", extra: true }",
            "#{ directory: \"../missing\" }",
            "#{ directory: \"root.rhai\" }",
            "#{ directory: \".\" }",
        ] {
            fs::write(&pointer, source).unwrap();
            assert!(resolve_root_directory(&directory, &registry).is_err(), "{source}");
        }
        fs::write(&pointer, "#{ directory: \"../target\" }").unwrap();
        let sibling = directory.join(".hidden");
        fs::write(&sibling, "contents").unwrap();
        assert!(resolve_root_directory(&directory, &registry).is_err());
        fs::remove_file(&sibling).unwrap();
        fs::create_dir(&sibling).unwrap();
        assert!(resolve_root_directory(&directory, &registry).is_err());
        fs::remove_dir(&sibling).unwrap();
        fs::write(target.join("root.rhai"), "#{ directory: \"..\" }").unwrap();
        assert!(resolve_root_directory(&directory, &registry).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn linked_rhai_pointers_use_their_physical_imports_and_target_directory() {
        let temporary = tempdir().unwrap();
        let directory = temporary.path().join("config");
        let physical = temporary.path().join("physical");
        let target = physical.join("target");
        fs::create_dir(&directory).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(physical.join("helpers.rhai"), "export let directory = \"target\";").unwrap();
        let pointer = physical.join("pointer.data");
        fs::write(
            &pointer,
            "import \"helpers.rhai\" as helpers; #{ directory: helpers::directory }",
        )
        .unwrap();
        std::os::unix::fs::symlink(&pointer, directory.join("root.rhai")).unwrap();
        let selected =
            resolve_root_directory(&directory, &default_format_parser_registry()).unwrap();
        assert_eq!(selected.canonicalize_path().unwrap(), target.canonicalize_path().unwrap());
    }
}
