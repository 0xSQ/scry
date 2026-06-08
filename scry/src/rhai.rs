//! Rhai scripting utilities for configuration.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use indexmap::IndexMap;
use rhai::{Dynamic, Engine, EvalAltResult, Module, ModuleResolver, Position, Scope};

use crate::BoxedError;

// ---------------------------------------------------------------------------------------------- //

/// Environment for evaluating Rhai scripts with constants and relative imports.
///
/// `RhaiEnv` manages constants that should be injected into all evaluated scripts
/// and their imported modules. It provides a module resolver that handles both
/// relative path resolution and constant injection.
///
/// # Example
///
/// ```ignore
/// use scry::RhaiEnv;
/// use rhai::Engine;
///
/// let mut env = RhaiEnv::new();
/// if let Some(config) = RhaiEnv::load_app_config("myapp", "env")? {
///     env.add_constant("env", config);
/// }
///
/// let mut engine = Engine::new();
/// engine.set_module_resolver(env.module_resolver());
///
/// let result = env.eval(&engine, &path)?;
/// ```
#[derive(Clone, Default)]
pub struct RhaiEnv {
    constants: IndexMap<String, Dynamic>,
}

impl RhaiEnv {
    /// Creates an empty environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads app config from `~/.config/{app_dir}/{filename}.rhai`.
    ///
    /// Returns `Ok(None)` if the config file doesn't exist.
    /// Returns an error if the file exists but fails to parse.
    ///
    /// The config file is evaluated with a fresh engine (no constants injected)
    /// to avoid circular dependencies.
    pub fn load_app_config(app_dir: &str, filename: &str) -> Result<Option<Dynamic>, RhaiError> {
        let home = dirs::home_dir().ok_or(RhaiError::NoHomeDir)?;
        let path = home.join(".config").join(app_dir).join(format!("{}.rhai", filename));

        if !path.is_file() {
            return Ok(None);
        }

        // Use a minimal engine without constants to avoid recursion
        let engine = Engine::new();
        let content = std::fs::read_to_string(&path).map_err(|e| RhaiError::ReadFile {
            path: path.clone(),
            source: e,
        })?;
        let mut ast = engine.compile(&content).map_err(|e| RhaiError::Compile {
            path: path.clone(),
            source: display_to_boxed(e),
        })?;
        ast.set_source(path.to_string_lossy().into_owned());

        let mut scope = Scope::new();
        let dynamic = engine.eval_ast_with_scope::<Dynamic>(&mut scope, &ast).map_err(|e| {
            RhaiError::EvalScript {
                path: path.clone(),
                source: display_to_boxed(*e),
            }
        })?;

        Ok(Some(dynamic))
    }

    /// Adds a constant to inject into all scripts and imported modules.
    pub fn add_constant(&mut self, name: impl Into<String>, value: Dynamic) -> &mut Self {
        self.constants.insert(name.into(), value);
        self
    }

    /// Creates a module resolver with relative imports and constant injection.
    pub fn module_resolver(&self) -> RelativeModuleResolver {
        RelativeModuleResolver {
            constants: self.constants.clone(),
        }
    }

    /// Creates a scope with all constants pre-injected.
    pub fn scope(&self) -> Scope<'static> {
        let mut scope = Scope::new();
        for (name, value) in &self.constants {
            scope.push_constant(name.clone(), value.clone());
        }
        scope
    }

    /// Evaluates a Rhai script file and returns the final expression.
    pub fn eval(&self, engine: &Engine, path: &Path) -> Result<Dynamic, RhaiError> {
        let content = std::fs::read_to_string(path).map_err(|e| RhaiError::ReadFile {
            path: path.to_path_buf(),
            source: e,
        })?;

        let mut ast = engine.compile(&content).map_err(|e| RhaiError::Compile {
            path: path.to_path_buf(),
            source: display_to_boxed(e),
        })?;
        ast.set_source(path.to_string_lossy().into_owned());

        let mut scope = self.scope();
        let result = engine.eval_ast_with_scope::<Dynamic>(&mut scope, &ast).map_err(|e| {
            RhaiError::EvalScript {
                path: path.to_path_buf(),
                source: display_to_boxed(*e),
            }
        })?;

        Ok(result)
    }

    /// Evaluates a Rhai script and extracts a named variable from the scope.
    pub fn eval_var(
        &self,
        engine: &Engine,
        path: &Path,
        var_name: &str,
    ) -> Result<Dynamic, RhaiError> {
        let content = std::fs::read_to_string(path).map_err(|e| RhaiError::ReadFile {
            path: path.to_path_buf(),
            source: e,
        })?;

        let mut ast = engine.compile(&content).map_err(|e| RhaiError::Compile {
            path: path.to_path_buf(),
            source: display_to_boxed(e),
        })?;
        ast.set_source(path.to_string_lossy().into_owned());

        let mut scope = self.scope();
        let _ = engine.eval_ast_with_scope::<Dynamic>(&mut scope, &ast).map_err(|e| {
            RhaiError::EvalScript {
                path: path.to_path_buf(),
                source: display_to_boxed(*e),
            }
        })?;

        match scope.get_value::<Dynamic>(var_name) {
            Some(value) => Ok(value),
            None => Err(RhaiError::VarNotFound {
                path: path.to_path_buf(),
                var: var_name.to_string(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------------------------- //

/// Module resolver that resolves imports relative to the importing file.
#[derive(Clone)]
pub struct RelativeModuleResolver {
    constants: IndexMap<String, Dynamic>,
}

impl ModuleResolver for RelativeModuleResolver {
    fn resolve(
        &self,
        engine: &Engine,
        source_path: Option<&str>,
        path: &str,
        pos: Position,
    ) -> Result<Rc<Module>, Box<EvalAltResult>> {
        // Determine base directory from the importing file's location
        let base_dir = source_path
            .map(|s| Path::new(s).parent().unwrap_or(Path::new(".")))
            .unwrap_or(Path::new("."));

        // Join with the import path (if path is absolute, it replaces base_dir)
        let target_path = base_dir.join(path);

        // Compile the module
        let ast = engine
            .compile_file(target_path.clone())
            .map_err(|e| Box::new(EvalAltResult::ErrorInModule(path.to_string(), e, pos)))?;

        // Create scope with constants injected
        let mut scope = Scope::new();
        for (name, value) in &self.constants {
            scope.push_constant(name.clone(), value.clone());
        }

        // Evaluate as a new module
        Module::eval_ast_as_new(scope, &ast, engine)
            .map(Rc::new)
            .map_err(|e| Box::new(EvalAltResult::ErrorInModule(path.to_string(), e, pos)))
    }
}

// ---------------------------------------------------------------------------------------------- //
// Rhai Evaluation Helpers

pub fn eval_str(s: &str) -> Result<Dynamic, RhaiError> {
    let engine = Engine::new();
    match engine.eval::<Dynamic>(s) {
        Ok(dynamic) => Ok(dynamic),
        Err(e) => Err(RhaiError::Eval {
            source: display_to_boxed(e),
        }),
    }
}

pub fn eval_script(path: impl AsRef<Path>) -> Result<Dynamic, RhaiError> {
    let path = path.as_ref();
    let env = RhaiEnv::new();
    let mut engine = Engine::new();
    engine.set_module_resolver(env.module_resolver());
    env.eval(&engine, path)
}

pub fn eval_script_var(path: impl AsRef<Path>, var: &str) -> Result<Dynamic, RhaiError> {
    let path = path.as_ref();
    let env = RhaiEnv::new();
    let mut engine = Engine::new();
    engine.set_module_resolver(env.module_resolver());
    env.eval_var(&engine, path, var)
}

// ---------------------------------------------------------------------------------------------- //
// RhaiError

/// Errors that occur during Rhai script evaluation.
#[derive(thiserror::Error, Debug)]
pub enum RhaiError {
    /// Failed to read the script file.
    #[error("failed to read Rhai script: {}", path.display())]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to compile the Rhai script.
    #[error("failed to compile Rhai script: {}", path.display())]
    Compile {
        path: PathBuf,
        #[source]
        source: BoxedError,
    },

    /// Failed to evaluate a string as a Rhai expression.
    #[error("failed to evaluate string as Rhai expression")]
    Eval {
        #[source]
        source: BoxedError,
    },

    /// Failed to evaluate the Rhai script.
    #[error("failed to evaluate Rhai script: {}", path.display())]
    EvalScript {
        path: PathBuf,
        #[source]
        source: BoxedError,
    },

    /// Requested variable not found in script.
    #[error("Rhai script '{}' does not contain variable '{var}'", path.display())]
    VarNotFound { path: PathBuf, var: String },

    /// Could not determine home directory for app config.
    #[error("could not determine home directory")]
    NoHomeDir,
}

// ---------------------------------------------------------------------------------------------- //
// Rhai Error Helpers

/// A simple string-based error wrapper for Display-only errors.
#[derive(Debug)]
struct StringError(String);

impl std::fmt::Display for StringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for StringError {}

/// Converts any Display-able value into a BoxedError.
///
/// Rhai errors implement Display but not std::error::Error, so we wrap them.
pub(crate) fn display_to_boxed(err: impl std::fmt::Display) -> BoxedError {
    Box::new(StringError(err.to_string()))
}
