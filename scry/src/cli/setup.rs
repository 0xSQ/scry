//! Builder and utilities for setting up clap-based CLI commands.
//!
//! The [`Setup`] builder creates CLI command [`Bundle`]s that can:
//! - Load configuration from files (with auto-discovery)
//! - Provide config description (--desc) and querying (--get)
//! - Apply CLI overrides (--set, --remove)
//!
//! Use [`Setup::into_bundle`] or [`Setup::into_bundle_with_matches`] to create bundles.

mod config_source;
mod error;
mod expose_map;
mod override_args;
mod query_args;
mod redirect;

use std::collections::HashSet;

use clap::{Arg, ArgMatches, Command};

use super::bundle::Bundle;
use crate::desc::Desc;
use crate::node::Node;
use crate::{Describe, FromNode};

pub use config_source::{
    find_config, find_config_default, find_config_with_registry, home_config_dir,
    load_rhai_var_node, ConfigPathPolicy, ConfigSource, DiscoverFn, FindConfigError,
    MissingConfigPolicy, MultiSourcePolicy, NodeLoaderFn, Required, ResolvedConfigInput,
};
pub use error::SetupError;
pub use expose_map::{ExposeEntry, ExposeKind, ExposeMap, Long};
pub use override_args::OverrideArgs;
pub use query_args::{handle_get_request, GetFormat, GetRequest, QueryArgs};
pub use redirect::{require_command_config, resolve_command_config, RedirectError, RedirectSpec};

// ---------------------------------------------------------------------------------------------- //

/// Builder type for setting up CLI commands using Scry's core traits.
///
/// The `Setup` builder is targeted towards setting up CLI commands for calling payload functions
/// whose main input type `T` implements the core Scry traits `FromNode` and `Describe`. We then
/// leverage these traits to automatically augment a CLI command with standard functionality
/// for inspecting, querying, and overriding the input configuration. In particular, we provide:
/// - Different ways to query and override the loaded config values
/// - Printing helpful descriptions of the config structure
/// - Different ways to locate and load the config file
///
/// Two constructors are available:
/// - [`Setup::new`]: Barebones default - no config source, no query/override args. Use this
///   for CLI-only commands where all input comes from exposed options and `--set`.
/// - [`Setup::standard`]: Batteries-included - positional `CONFIG` argument, standard query
///   and override options. Use this as the go-to default for config-driven commands.
///
/// You can call the builder methods to customize these defaults and then call the `into_bundle`
/// methods to create a runnable `Bundle` instance. The bundle's handler function then calls
/// the user payload function with an instance of its input target type, `T`.
///
/// The handler function returns a result of type `Bundle<Option<R>, SetupError>`.
/// The `Option` distinguishes whether the user payload function was executed or skipped
/// (e.g., due to the use of query options). Framework-level errors (config loading,
/// overrides, etc.) are reported via the `SetupError` type.
pub struct Setup {
    pub(crate) command_name: String,
    pub(crate) about: Option<String>,
    pub(crate) config_source: Option<ConfigSource>,
    pub(crate) override_args: OverrideArgs,
    pub(crate) query_args: QueryArgs,
    pub(crate) expose_map: ExposeMap,
    pub(crate) extra_args: Vec<Arg>,
}

// ---------------------------------------------------------------------------------------------- //

impl Setup {
    /// Creates a barebones builder with no config source and no query/override args.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            command_name: name.into(),
            about: None,
            config_source: None,
            override_args: OverrideArgs::default(),
            query_args: QueryArgs::default(),
            expose_map: ExposeMap::default(),
            extra_args: Vec::new(),
        }
    }

    /// Creates a batteries-included builder with standard config source and query/override args.
    ///
    /// Defaults:
    /// - Positional `CONFIG` argument for the config file path
    /// - `Node::parse_file` as the loader
    /// - Standard query and override options (`--desc`, `--get`, `--set`, etc.)
    pub fn standard(name: impl Into<String>) -> Self {
        Self {
            command_name: name.into(),
            about: None,
            config_source: Some(ConfigSource::new().positional(
                "CONFIG",
                "Path to config file",
                Required::Yes,
            )),
            override_args: OverrideArgs::standard(),
            query_args: QueryArgs::standard(),
            expose_map: ExposeMap::default(),
            extra_args: Vec::new(),
        }
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

    /// Sets override arguments (--set, --remove).
    ///
    /// If not called, defaults depend on the constructor:
    /// - `new()`: No override args.
    /// - `standard()`: Standard override args (`--set`, `--remove`).
    pub fn override_args(mut self, args: OverrideArgs) -> Self {
        self.override_args = args;
        self
    }

    /// Sets query arguments (--desc, --get, --get-flat, --get-as).
    ///
    /// If not called, defaults depend on the constructor:
    /// - `new()`: No query args.
    /// - `standard()`: Standard query args (`--desc`, `--get`, etc.).
    pub fn query_args(mut self, args: QueryArgs) -> Self {
        self.query_args = args;
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

    /// Adds a raw clap Arg for CLI-only arguments not in config.
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
    /// Panics if argument names collide (duplicate IDs, long options, or short flags).
    pub fn into_bundle<T, R>(
        self,
        func: impl FnOnce(T) -> R + 'static,
    ) -> Bundle<Option<R>, SetupError>
    where
        T: FromNode + Describe + 'static,
        R: 'static,
    {
        let cmd = self.build_command(&T::describe());
        Bundle::single(cmd, move |matches| match self.prepare_input::<T>(matches)? {
            Some(input) => Ok(Some(func(input))),
            None => Ok(None),
        })
    }

    /// Creates a bundle with a function that takes the target type and CLI matches.
    ///
    /// # Panics
    ///
    /// Panics if argument names collide (duplicate IDs, long options, or short flags).
    pub fn into_bundle_with_matches<T, R>(
        self,
        func: impl FnOnce(T, &ArgMatches) -> R + 'static,
    ) -> Bundle<Option<R>, SetupError>
    where
        T: FromNode + Describe + 'static,
        R: 'static,
    {
        let cmd = self.build_command(&T::describe());
        Bundle::single(cmd, move |matches| match self.prepare_input::<T>(matches)? {
            Some(input) => Ok(Some(func(input, matches))),
            None => Ok(None),
        })
    }

    // ---------------------------------------------------------------------------------------------- //
    // Helpers

    /// Builds the clap Command from the current setup configuration.
    ///
    /// # Panics
    ///
    /// Panics if argument names collide (duplicate IDs, long options, or short flags).
    fn build_command(&self, desc: &Desc) -> Command {
        if let Err(e) = check_collisions(
            self.config_source.as_ref(),
            &self.override_args,
            &self.query_args,
            &self.expose_map,
            &self.extra_args,
        ) {
            panic!("CLI argument collision: {e}");
        }

        let mut cmd = Command::new(self.command_name.clone());
        if let Some(about) = &self.about {
            cmd = cmd.about(about.clone());
        }
        if let Some(source) = &self.config_source {
            cmd = source.augment(cmd, &self.query_args);
        }
        cmd = self.override_args.augment(cmd);
        cmd = self.query_args.augment(cmd);
        cmd = self.expose_map.augment(cmd, desc);
        for arg in &self.extra_args {
            cmd = cmd.arg(arg.clone());
        }
        cmd
    }

    /// Prepares an instance of the payload input type `T`.
    ///
    /// Returns `Ok(None)` if a query command (--desc, --get) was handled.
    /// Returns `Ok(Some(input))` if the input is ready for the user's function.
    fn prepare_input<T: FromNode + Describe>(
        &self,
        matches: &ArgMatches,
    ) -> Result<Option<T>, SetupError> {
        // Handle --desc (no config file needed).
        if let Some(output) = self.query_args.check_desc::<T>(matches)? {
            println!("{}", output);
            return Ok(None);
        }

        // Load config node from file or start with an empty map.
        let mut node = match &self.config_source {
            Some(source) => match source.resolve_input(&self.command_name, matches)? {
                ResolvedConfigInput::Path(path) => {
                    (source.loader)(&path).map_err(|e| SetupError::LoadConfigFile {
                        path: path.to_path_buf(),
                        source: e,
                    })?
                }
                ResolvedConfigInput::Empty => Node::default(),
            },
            None => Node::default(),
        };

        // Apply overrides.
        self.override_args.apply::<T>(&mut node, &self.expose_map, matches)?;

        // Handle --get variants.
        if let Some(output) = self.query_args.check_get::<T>(&node, matches)? {
            println!("{}", output);
            return Ok(None);
        }

        let input: T = T::from_node(&node).map_err(|e| SetupError::EvaluateInputConfig {
            source: Box::new(e),
        })?;

        Ok(Some(input))
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
// Collision Detection

/// Checks for argument name collisions across all configured arguments.
///
/// Returns an error if any argument IDs, long options, or short flags collide.
pub fn check_collisions(
    config_source: Option<&ConfigSource>,
    override_args: &OverrideArgs,
    query_args: &QueryArgs,
    expose: &ExposeMap,
    extra_args: &[Arg],
) -> Result<(), ArgCollisionError> {
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut seen_longs: HashSet<String> = HashSet::new();
    let mut seen_shorts: HashSet<char> = HashSet::new();

    if let Some(config) = config_source {
        // Config positional arg (uses name as ID, no long flag).
        if let Some(pos) = &config.positional {
            seen_ids.insert(pos.name.clone());
        }

        // Config option arg (ID = long).
        if let Some(opt) = &config.option {
            if !seen_ids.insert(opt.long.clone()) {
                return Err(ArgCollisionError::Id(opt.long.clone()));
            }
            if !seen_longs.insert(opt.long.clone()) {
                return Err(ArgCollisionError::Long(opt.long.clone()));
            }
            if let Some(short) = opt.short {
                if !seen_shorts.insert(short) {
                    return Err(ArgCollisionError::Short(short));
                }
            }
        }
    }

    // Override args (ID = long).
    for name in override_args.all_names() {
        let name_str = name.to_string();
        if !seen_ids.insert(name_str.clone()) {
            return Err(ArgCollisionError::Id(name_str));
        }
        if !seen_longs.insert(name.to_string()) {
            return Err(ArgCollisionError::Long(name.to_string()));
        }
    }
    for short in override_args.all_shorts() {
        if !seen_shorts.insert(short) {
            return Err(ArgCollisionError::Short(short));
        }
    }

    // Query args (ID = long).
    for name in query_args.all_names() {
        let name_str = name.to_string();
        if !seen_ids.insert(name_str.clone()) {
            return Err(ArgCollisionError::Id(name_str));
        }
        if !seen_longs.insert(name.to_string()) {
            return Err(ArgCollisionError::Long(name.to_string()));
        }
    }
    for short in query_args.all_shorts() {
        if !seen_shorts.insert(short) {
            return Err(ArgCollisionError::Short(short));
        }
    }

    // Exposed entries (ID = arg_name).
    for entry in &expose.entries {
        let arg_name = entry.arg_name();
        if !seen_ids.insert(arg_name.clone()) {
            return Err(ArgCollisionError::Id(arg_name));
        }
        if !matches!(entry.long, Long::None) && !seen_longs.insert(arg_name.clone()) {
            return Err(ArgCollisionError::Long(arg_name));
        }
        if let Some(short) = entry.short {
            if !seen_shorts.insert(short) {
                return Err(ArgCollisionError::Short(short));
            }
        }
    }

    // User's extra args (ID and long are separate).
    for arg in extra_args {
        let id = arg.get_id().as_str().to_string();
        if !seen_ids.insert(id.clone()) {
            return Err(ArgCollisionError::Id(id));
        }
        if let Some(long) = arg.get_long() {
            let long_str = long.to_string();
            if !seen_longs.insert(long_str.clone()) {
                return Err(ArgCollisionError::Long(long_str));
            }
        }
        if let Some(short) = arg.get_short() {
            if !seen_shorts.insert(short) {
                return Err(ArgCollisionError::Short(short));
            }
        }
    }

    Ok(())
}

/// Errors from argument name/id collisions when building a CLI command.
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
