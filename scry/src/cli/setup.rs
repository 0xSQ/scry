//! Builds commands around a shared configuration input pipeline.
//!
//! [`Setup`] combines input sources, exposed arguments, and configuration support tools.
//! Every command includes a `--scry` / `-S` gateway for inspecting and editing its input.
//!
//! Use [`Setup::into_bundle`] or [`Setup::into_bundle_with_matches`] to create bundles.

mod config_source;
mod edits;
mod error;
mod expose_map;
mod gateway;
mod redirect;
mod rhai_env;

use std::collections::HashSet;

use clap::{Arg, ArgMatches, Command};
use indexmap::IndexMap;

use super::bundle::Bundle;
use crate::desc::{Desc, DescKind, DescPathError, DisplayConfig, VariantRepr};
use crate::key_path::Segment;
use crate::node::{Format, Kind, Node};
use crate::writer::TreeConfig;
use crate::{Describe, FromNode, KeyPath};

pub use config_source::{
    find_config, find_config_default, find_config_with_registry, home_config_dir,
    load_rhai_var_node, ConfigPathPolicy, ConfigSource, DiscoverFn, FindConfigError,
    MissingConfigPolicy, MultiSourcePolicy, NodeLoaderFn, ResolvedConfigInput,
};
pub use error::SetupError;
pub use expose_map::{
    Arity, ExposeEntry, ExposeKind, ExposeMap, FixedAssignment, Long, ValueShape,
};
pub use gateway::Gateway;
pub use redirect::{require_command_config, resolve_command_config, RedirectError, RedirectSpec};
pub use rhai_env::load_app_rhai_env;

// ---------------------------------------------------------------------------------------------- //

/// Configures a command's input and configuration support tools.
///
/// The input type implements [`FromNode`] and [`Describe`]. A [`ConfigSource`] supplies the
/// initial [`Node`], and exposed arguments edit it. A [`Gateway`] provides configuration tools
/// with fixed operation names.
///
/// Each invocation first selects its action. Help and descriptions finish without loading
/// input. Other actions load the input and apply command-line edits, then either inspect the
/// raw Node or decode it for the payload function.
///
/// [`Setup::new`] starts with an empty Node. [`Setup::standard`] accepts a config file through
/// `-C` / `--config-file`. Both include the default gateway. Builder methods customize either
/// starting point before [`into_bundle`](Self::into_bundle) creates a command.
///
/// Running the resulting bundle returns `Result<Option<R>, SetupError>`. `Some(R)` holds the
/// payload's result. `None` means a support action completed without running the payload.
pub struct Setup {
    command_name: String,
    about: Option<String>,
    config_source: Option<ConfigSource>,
    gateway: Gateway,
    expose_map: ExposeMap,
    extra_args: Vec<Arg>,
}

// ---------------------------------------------------------------------------------------------- //

impl Setup {
    /// Creates a command with the default gateway and an empty input Node.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            command_name: name.into(),
            about: None,
            config_source: None,
            gateway: Gateway::default(),
            expose_map: ExposeMap::default(),
            extra_args: Vec::new(),
        }
    }

    /// Creates a command that loads input through `-C` / `--config-file`.
    ///
    /// The source uses [`Node::parse_file`] and reports an error when an input action has no path.
    /// Help and description actions do not need a config file.
    pub fn standard(name: impl Into<String>) -> Self {
        Self::new(name)
            .config_source(|source| source.option("config-file", Some('C'), "Path to config file."))
    }

    /// Sets the command description shown in --help.
    pub fn about(mut self, text: impl Into<String>) -> Self {
        self.about = Some(text.into());
        self
    }

    /// Configures where and how to load the config file.
    ///
    /// The closure receives the current `ConfigSource` (or a fresh default if none was set)
    /// and returns the modified version.
    pub fn config_source(mut self, f: impl FnOnce(ConfigSource) -> ConfigSource) -> Self {
        let current = self.config_source.take().unwrap_or_default();
        self.config_source = Some(f(current));
        self
    }

    /// Removes the config source, switching to CLI-only mode.
    ///
    /// An empty map node is used as the starting point instead of loading from a file.
    pub fn no_config_source(mut self) -> Self {
        self.config_source = None;
        self
    }

    /// Configures the gateway flag names.
    ///
    /// The default flags are `--scry` and `-S`. Application arguments come before the flag.
    /// The suffix accepts `--set`, `--remove`, `get`, and `desc`. Edits come before `get`,
    /// and an edit-only suffix runs the payload with the edited input.
    ///
    /// [`Gateway`] configures the long flag name and optional short alias. The operation names
    /// within the suffix are fixed.
    ///
    /// Application arguments are optional in the parser. Source resolution and typed decoding
    /// enforce the payload's input requirements.
    ///
    /// ```
    /// use scry::cli::setup::{Gateway, Setup};
    ///
    /// let setup = Setup::new("resize").gateway(Gateway::new().long("look"));
    /// ```
    pub fn gateway(mut self, gateway: Gateway) -> Self {
        self.gateway = gateway;
        self
    }

    /// Exposes config fields as CLI arguments.
    ///
    /// The closure receives a mutable reference to the current `ExposeMap`.
    /// Fields can be added incrementally across multiple calls.
    pub fn expose(mut self, f: impl FnOnce(&mut ExposeMap)) -> Self {
        f(&mut self.expose_map);
        self
    }

    /// Adds an optional clap argument outside the configuration input.
    ///
    /// The payload can read it through [`into_bundle_with_matches`](Self::into_bundle_with_matches).
    pub fn arg(mut self, arg: Arg) -> Self {
        self.extra_args.push(arg);
        self
    }

    // ---------------------------------------------------------------------------------------------- //
    // Bundle Creation

    /// Creates a bundle with a function that takes the target type.
    ///
    /// # Panics
    ///
    /// Panics if argument names collide (duplicate IDs, long options, or short flags), an exposure
    /// declaration is invalid, or a positional array uses an unsupported positional layout.
    /// Application arguments must be optional so support actions can run without payload input.
    pub fn into_bundle<T, R>(
        self,
        func: impl FnOnce(T) -> R + 'static,
    ) -> Bundle<Option<R>, SetupError>
    where
        T: FromNode + Describe + 'static,
        R: 'static,
    {
        self.into_bundle_with_matches(move |input, _| func(input))
    }

    /// Creates a bundle with a function that takes the target type and CLI matches.
    ///
    /// # Panics
    ///
    /// Panics if argument names collide (duplicate IDs, long options, or short flags), an exposure
    /// declaration is invalid, or a positional array uses an unsupported positional layout.
    /// Application arguments must be optional so support actions can run without payload input.
    pub fn into_bundle_with_matches<T, R>(
        self,
        func: impl FnOnce(T, &ArgMatches) -> R + 'static,
    ) -> Bundle<Option<R>, SetupError>
    where
        T: FromNode + Describe + 'static,
        R: 'static,
    {
        let cmd = self.build_command(&T::describe());
        Bundle::single_with_command(cmd, move |matches, command| {
            match self.prepare_input::<T>(matches, command)? {
                Some(input) => Ok(Some(func(input, matches))),
                None => Ok(None),
            }
        })
    }

    // ---------------------------------------------------------------------------------------------- //
    // Helpers

    /// Builds the clap Command from the current setup configuration.
    ///
    /// # Panics
    ///
    /// Panics if argument names collide (duplicate IDs, long options, or short flags), an exposure
    /// declaration is invalid, or a positional array uses an unsupported positional layout.
    fn build_command(&self, desc: &Desc) -> Command {
        let mut cmd = Command::new(self.command_name.clone());
        if let Some(about) = &self.about {
            cmd = cmd.about(about.clone());
        }
        if let Some(source) = &self.config_source {
            cmd = source.augment(cmd);
        }
        cmd = self.expose_map.augment(cmd, desc);
        for arg in &self.extra_args {
            cmd = cmd.arg(arg.clone());
        }
        cmd = self.gateway.augment(cmd);
        if let Err(error) = check_collisions(&cmd) {
            panic!("CLI argument collision: {error}");
        }
        assert!(
            cmd.get_arguments().all(|arg| !arg.is_required_set()),
            "Setup arguments must be optional. Validate payload requirements during execution"
        );
        validate_positional_arrays(&cmd, &self.expose_map);
        cmd
    }

    /// Prepares an instance of the payload input type `T`.
    ///
    /// Returns `Ok(None)` if a support action was handled.
    /// Returns `Ok(Some(input))` if the input is ready for the user's function.
    fn prepare_input<T: FromNode + Describe>(
        &self,
        matches: &ArgMatches,
        command: &Command,
    ) -> Result<Option<T>, SetupError> {
        let invocation = self.gateway.parse(matches, command)?;
        match &invocation.action {
            Action::Help(output) => {
                output.print().map_err(|source| SetupError::PrintHelp { source })?;
                return Ok(None);
            }
            Action::Describe(request) => {
                println!(
                    "{}",
                    format_description_with_config::<T>(&request.path, &request.config)?
                );
                return Ok(None);
            }
            Action::Execute | Action::Get(_) => {}
        }

        let mut node = self.load_node(matches)?;
        self.expose_map.apply::<T>(&mut node, matches)?;
        edits::apply::<T>(&mut node, invocation.edits)?;
        if let Action::Get(request) = invocation.action {
            println!("{}", format_node_at_path::<T>(&node, &request.path, request.format)?);
            return Ok(None);
        }

        T::from_node(&node).map(Some).map_err(|error| SetupError::EvaluateInputConfig {
            source: Box::new(error),
        })
    }

    fn load_node(&self, matches: &ArgMatches) -> Result<Node, SetupError> {
        let Some(source) = &self.config_source else {
            return Ok(Node::empty_map());
        };
        match source.resolve_input(&self.command_name, matches)? {
            ResolvedConfigInput::Path(path) => {
                (source.loader)(&path).map_err(|error| SetupError::LoadConfigFile {
                    path,
                    source: error,
                })
            }
            ResolvedConfigInput::Empty => Ok(Node::empty_map()),
        }
    }
}
// ---------------------------------------------------------------------------------------------- //
// Setup Bundle

pub type SetupBundle<R> = Bundle<Option<R>, SetupError>;

/// Creates a bundle with a handler function whose output is always wrapped in `Option::Some`.
///
/// Use this factory function if you want to mix user payload function that are built through the
/// [`Setup`] builder with other bundles that are created manually. It creates a bundle where the
/// payload function's return type is lifted into `Option<R>::Some` so that it matches the common
/// return type of bundles created via `Setup`.
pub fn create_setup_bundle<R, F>(command: Command, func: F) -> SetupBundle<R>
where
    R: 'static,
    F: FnOnce(&ArgMatches) -> R + 'static,
{
    Bundle::single(command, move |matches| Ok(Some(func(matches))))
}

// ---------------------------------------------------------------------------------------------- //
// Gateway Invocation

struct Invocation {
    action: Action,
    edits: Vec<edits::Edit>,
}

enum Action {
    Execute,
    Describe(DescRequest),
    Get(GetRequest),
    Help(clap::Error),
}

// ---------------------------------------------------------------------------------------------- //
// Input Inspection

/// A description inspection request.
struct DescRequest {
    /// The description path to retrieve.
    path: String,
    /// The display settings for the selected branch.
    config: DisplayConfig,
}

/// An input inspection request.
struct GetRequest {
    /// The key path to retrieve.
    path: String,
    /// The output format.
    format: GetFormat,
}

/// Selects the output view for input inspection.
#[derive(Clone)]
pub enum GetFormat {
    /// Tree-structured output with the selected display settings.
    Tree(TreeConfig),
    /// Flat key-value output with a space between each path and value.
    Flat,
    /// Format-specific serialized output.
    Format(Format),
}

/// Formats the complete description or one validated branch.
pub fn format_description<T: Describe>(path: &str) -> Result<String, SetupError> {
    format_description_with_config::<T>(path, &DisplayConfig::default())
}

/// Formats a description with depth counted from the selected branch's children.
pub fn format_description_with_config<T: Describe>(
    path: &str,
    config: &DisplayConfig,
) -> Result<String, SetupError> {
    let desc = T::describe();
    if path.is_empty() {
        return Ok(desc.display_with(config));
    }
    desc.validate_path(path).map_err(|source| SetupError::DescPath { source })?;
    let entry = desc.entry_at_path(path).expect("validated description path must exist");
    let mut config = config.clone();
    // The selected entry's heading is outside the requested child levels.
    config.max_depth = config.max_depth.map(|depth| depth.saturating_add(1));
    Ok(entry.display_with(&config))
}

/// Formats the input Node or a selected subtree in declaration order.
///
/// Known fields follow their description's order. Unknown keys retain their stored order after
/// known fields, and values without a matching description retain their stored order throughout.
/// Formatting leaves the input's values, paths, order, and visitation state unchanged.
pub fn format_node_at_path<T>(
    node: &Node,
    path: &str,
    format: GetFormat,
) -> Result<String, SetupError>
where
    T: Describe,
{
    let key_path = if path.is_empty() {
        KeyPath::new()
    } else {
        path.parse().map_err(|e| SetupError::KeyPath { source: e })?
    };

    let Some(target) = node.opt_node(&key_path)? else {
        // Path not in tree. Check desc for a helpful hint, then return an appropriate error.
        let mut message = format!("cannot get '{path}': path does not exist");
        let desc = T::describe();
        if let Err(DescPathError::UnknownPath(upe)) = desc.validate_path(path) {
            message.push_str(&format!("\n\n{upe}"));
        }
        return Err(SetupError::GetNotFound { message });
    };

    let desc = T::describe();
    let target = ordered_query_node(target, query_description_at_path(node, &desc, &key_path));

    match format {
        GetFormat::Tree(config) => Ok(target.to_tree_string_with_config(config)),
        GetFormat::Flat => Ok(target.to_flat_string()),
        GetFormat::Format(format_id) => {
            let registry = crate::node::default_format_writer_registry();
            Ok(target.to_format_string(format_id, &registry)?)
        }
    }
}

fn query_description_at_path<'a>(
    mut node: &Node,
    mut desc: &'a Desc,
    path: &KeyPath,
) -> Option<&'a Desc> {
    for segment in path.iter() {
        (node, desc) = match (segment, &node.kind, &desc.kind) {
            (Segment::Key(key), Kind::Map(map), DescKind::Struct { fields }) => {
                (map.get(key)?, &fields.iter().find(|field| field.name == *key)?.value)
            }
            (Segment::Key(key), Kind::Map(map), DescKind::Enum { .. }) if map.len() == 1 => {
                (map.get(key)?, query_payload_description(desc, key)?)
            }
            (Segment::Index(index), Kind::Vec(items), DescKind::List { item }) => {
                (items.get(*index)?, item.as_ref())
            }
            (Segment::Index(index), Kind::Vec(values), DescKind::Tuple { items }) => {
                (values.get(*index)?, items.get(*index)?)
            }
            _ => return None,
        };
    }
    Some(desc)
}

fn query_payload_description<'a>(desc: &'a Desc, key: &str) -> Option<&'a Desc> {
    let DescKind::Enum { variants } = &desc.kind else {
        return None;
    };
    match &variants.iter().find(|variant| variant.name == key)?.repr {
        VariantRepr::Payload { payload, .. } => Some(payload),
        VariantRepr::Unit { .. } => None,
    }
}

fn ordered_query_node(node: &Node, desc: Option<&Desc>) -> Node {
    match &node.kind {
        Kind::Leaf(leaf) => Node::new_leaf(node.path.clone(), leaf.value.clone()),
        Kind::Vec(values) => {
            let children = values
                .iter()
                .enumerate()
                .map(|(index, child)| {
                    let child_desc = match desc.map(|desc| &desc.kind) {
                        Some(DescKind::List { item }) => Some(item.as_ref()),
                        Some(DescKind::Tuple { items }) => items.get(index),
                        _ => None,
                    };
                    ordered_query_node(child, child_desc)
                })
                .collect();
            Node::new_vec(node.path.clone(), children)
        }
        Kind::Map(values) => {
            let mut children = IndexMap::with_capacity(values.len());
            if let Some(DescKind::Struct { fields }) = desc.map(|desc| &desc.kind) {
                for field in fields {
                    if !children.contains_key(&field.name) {
                        if let Some(child) = values.get(&field.name) {
                            children.insert(
                                field.name.clone(),
                                ordered_query_node(child, Some(&field.value)),
                            );
                        }
                    }
                }
            }
            for (key, child) in values {
                if !children.contains_key(key) {
                    let child_desc = desc
                        .filter(|_| values.len() == 1)
                        .and_then(|desc| query_payload_description(desc, key));
                    children.insert(key.clone(), ordered_query_node(child, child_desc));
                }
            }
            Node::new_map(node.path.clone(), children)
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Command Validation

/// Checks an assembled command for conflicting argument names.
///
/// Validates argument IDs, long options, short flags, and their aliases. Built-in help and version
/// names are reserved when enabled. Call this before clap builds the command or parses arguments.
pub fn check_collisions(command: &Command) -> Result<(), ArgCollisionError> {
    let mut ids = HashSet::new();
    let mut longs = HashSet::new();
    let mut shorts = HashSet::new();

    if !command.is_multicall_set() {
        if !command.is_disable_help_flag_set() {
            ids.insert("help");
            longs.insert("help");
            shorts.insert('h');
        }
        if !command.is_disable_version_flag_set() {
            ids.insert("version");
            longs.insert("version");
            shorts.insert('V');
        }
    }

    for arg in command.get_arguments() {
        let id = arg.get_id().as_str();
        if !ids.insert(id) {
            return Err(ArgCollisionError::Id(id.to_owned()));
        }
        for long in arg.get_long().into_iter().chain(arg.get_all_aliases().into_iter().flatten()) {
            if !longs.insert(long) {
                return Err(ArgCollisionError::Long(long.to_owned()));
            }
        }
        for short in
            arg.get_short().into_iter().chain(arg.get_all_short_aliases().into_iter().flatten())
        {
            if !shorts.insert(short) {
                return Err(ArgCollisionError::Short(short));
            }
        }
    }
    Ok(())
}

/// Reports conflicting argument names in an assembled command.
#[derive(Debug, thiserror::Error)]
pub enum ArgCollisionError {
    /// Two arguments have the same argument ID.
    #[error("duplicate arg id: {0}")]
    Id(String),
    /// Two arguments have the same long option name.
    #[error("duplicate long option: --{0}")]
    Long(String),
    /// Two arguments have the same short option character.
    #[error("duplicate short option: -{0}")]
    Short(char),
}

// ---------------------------------------------------------------------------------------------- //

/// Validates the supported exposed positional array layout.
fn validate_positional_arrays(command: &Command, expose: &ExposeMap) {
    let arrays = expose
        .entries
        .iter()
        .filter(|entry| {
            matches!(entry.kind, ExposeKind::Positional { .. }) && entry.shape == ValueShape::Array
        })
        .count();
    if arrays > 0 {
        assert!(arrays == 1, "only one exposed positional array is supported");
        assert!(
            command.get_positionals().count() == 1,
            "an exposed positional array must be the command's only positional argument"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Value;

    #[test]
    fn query_copy_has_independent_leaf_visitation() {
        let original = Node::new_vec(
            KeyPath::new(),
            vec![
                Node::new_leaf(KeyPath::from_index(0), Value::U32(1)),
                Node::new_leaf(KeyPath::from_index(1), Value::Null),
            ],
        );
        original.req_node("[0]").unwrap().as_leaf().unwrap().mark_visited();

        let copy = ordered_query_node(&original, None);
        assert!(!copy.req_node("[0]").unwrap().as_leaf().unwrap().is_visited());
        copy.req_node("[1]").unwrap().as_leaf().unwrap().mark_visited();
        assert!(original.req_node("[0]").unwrap().as_leaf().unwrap().is_visited());
        assert!(!original.req_node("[1]").unwrap().as_leaf().unwrap().is_visited());
    }
}
