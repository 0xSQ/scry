//! Rhai scripting utilities for configuration.

use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use indexmap::IndexMap;
use rhai::{Dynamic, Engine, EvalAltResult, Module, ModuleResolver, ParseError, Position, Scope};

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
/// use rhai::Engine;
///
/// let env = scry::cli::setup::load_app_rhai_env("myapp")?;
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

    /// Loads a named local Rhai file from `~/.config/{app_dir}/{filename}.rhai`.
    ///
    /// Returns `Ok(None)` if the config file doesn't exist.
    /// Returns an error if the file exists but fails to parse.
    ///
    /// The config file is evaluated with a fresh engine (no constants injected)
    /// to avoid circular dependencies.
    /// This low-level helper reads that local file directly, without following `source.*`.
    /// Application environments also stay local and support all enabled config formats through
    /// [`crate::cli::setup::load_app_rhai_env`] or [`crate::cli::setup::ApplicationConfig`].
    pub fn load_app_config(app_dir: &str, filename: &str) -> Result<Option<Dynamic>, RhaiError> {
        let home = dirs::home_dir().ok_or(RhaiError::NoHomeDir)?;
        let path = home.join(".config").join(app_dir).join(format!("{}.rhai", filename));

        if !path.is_file() {
            return Ok(None);
        }

        // Uses a minimal engine without constants to avoid recursion.
        let engine = Engine::new();
        Self::new().eval(&engine, &path).map(Some)
    }

    /// Adds a constant to inject into all scripts and imported modules.
    pub fn add_constant(&mut self, name: impl Into<String>, value: Dynamic) -> &mut Self {
        self.constants.insert(name.into(), value);
        self
    }

    /// Creates a module resolver with relative imports and constant injection.
    pub fn module_resolver(&self) -> RelativeModuleResolver {
        RelativeModuleResolver { env: self.clone() }
    }

    /// Creates a scope with all constants pre-injected.
    pub fn scope(&self) -> Scope<'static> {
        let mut scope = Scope::new();
        for (name, value) in &self.constants {
            scope.push_constant(name.clone(), value.clone());
        }
        scope
    }

    /// Evaluates a Rhai script file and returns its result.
    pub fn eval(&self, engine: &Engine, path: &Path) -> Result<Dynamic, RhaiError> {
        self.eval_with_scope(engine, path).map(|(value, _)| value)
    }

    /// Evaluates a Rhai script and extracts a named variable from the scope.
    pub fn eval_var(
        &self,
        engine: &Engine,
        path: &Path,
        var_name: &str,
    ) -> Result<Dynamic, RhaiError> {
        let (_, scope) = self.eval_with_scope(engine, path)?;
        scope.get_value::<Dynamic>(var_name).ok_or_else(|| RhaiError::VarNotFound {
            path: path.to_path_buf(),
            var: var_name.to_string(),
        })
    }

    fn eval_with_scope(
        &self,
        engine: &Engine,
        path: &Path,
    ) -> Result<(Dynamic, Scope<'static>), RhaiError> {
        let path = canonical_script_path(path)?;
        let content = std::fs::read_to_string(&path).map_err(|e| RhaiError::ReadFile {
            path: path.to_path_buf(),
            source: e,
        })?;

        let mut ast = engine.compile(&content).map_err(|e| RhaiError::Compile {
            path: path.to_path_buf(),
            source: Box::new(ScriptError::from_parse(&path, &content, &e)),
        })?;
        ast.set_source(path.to_string_lossy().into_owned());

        let mut scope = self.scope();
        let value = engine.eval_ast_with_scope::<Dynamic>(&mut scope, &ast).map_err(|e| {
            RhaiError::EvalScript {
                path: path.to_path_buf(),
                source: Box::new(ScriptError::from_eval(&path, &e)),
            }
        })?;

        Ok((value, scope))
    }
}

// ---------------------------------------------------------------------------------------------- //

/// Resolves imports relative to each importing file's canonical target.
#[derive(Clone)]
pub struct RelativeModuleResolver {
    env: RhaiEnv,
}

impl ModuleResolver for RelativeModuleResolver {
    fn resolve(
        &self,
        engine: &Engine,
        source_path: Option<&str>,
        path: &str,
        pos: Position,
    ) -> Result<Rc<Module>, Box<EvalAltResult>> {
        // Determines the base directory from the importing file's canonical location.
        let base_dir = source_path
            .map(|s| Path::new(s).parent().unwrap_or(Path::new(".")))
            .unwrap_or(Path::new("."));

        // An absolute import path replaces the importing file's directory.
        let requested_path = base_dir.join(path);
        let target_path = canonical_script_path(&requested_path).map_err(|error| {
            Box::new(EvalAltResult::ErrorInModule(
                requested_path.display().to_string(),
                Box::new(EvalAltResult::ErrorSystem(error.to_string(), Box::new(error))),
                pos,
            ))
        })?;

        // Report the resolved path in errors so readers (and snippet rendering) can locate the
        // module file without re-deriving the import resolution.
        let module_name = target_path.display().to_string();

        // Compile the module
        let ast = engine
            .compile_file(target_path.clone())
            .map_err(|e| Box::new(EvalAltResult::ErrorInModule(module_name.clone(), e, pos)))?;

        // Evaluate as a new module
        Module::eval_ast_as_new(self.env.scope(), &ast, engine)
            .map(Rc::new)
            .map_err(|e| Box::new(EvalAltResult::ErrorInModule(module_name, e, pos)))
    }
}

// ---------------------------------------------------------------------------------------------- //
// Rhai Evaluation Helpers

pub fn eval_str(s: &str) -> Result<Dynamic, RhaiError> {
    let engine = Engine::new();
    engine.eval::<Dynamic>(s).map_err(|e| RhaiError::Eval {
        source: Box::new(ScriptError::from_str_eval(s, &e)),
    })
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
    /// Failed to resolve the script's physical filesystem target.
    #[error("failed to resolve Rhai script: {}", path.display())]
    ResolveFile {
        path: PathBuf,
        #[source]
        source: crate::util::PathError,
    },

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
        source: Box<ScriptError>,
    },

    /// Failed to evaluate a string as a Rhai expression.
    #[error("failed to evaluate string as Rhai expression")]
    Eval {
        #[source]
        source: Box<ScriptError>,
    },

    /// Failed to evaluate the Rhai script.
    #[error("failed to evaluate Rhai script: {}", path.display())]
    EvalScript {
        path: PathBuf,
        #[source]
        source: Box<ScriptError>,
    },

    /// Requested variable not found in script.
    #[error("Rhai script '{}' does not contain variable '{var}'", path.display())]
    VarNotFound { path: PathBuf, var: String },

    /// Could not determine home directory for app config.
    #[error("could not determine home directory")]
    NoHomeDir,
}

// ---------------------------------------------------------------------------------------------- //
// ScriptError

/// A Rhai script error with an optional source location for snippet rendering.
///
/// Rhai's own error types are not `Send + Sync` (they can contain `Dynamic` values), so the
/// relevant data is extracted into owned fields at construction time. The `Display` output is
/// the original Rhai message followed by a compiler-style source snippet when a location could
/// be captured.
#[derive(Debug, Clone)]
pub struct ScriptError {
    /// The original Rhai error message.
    pub message: String,
    /// Where the error occurred, if Rhai reported a usable position.
    pub location: Option<SourceLocation>,
}

impl ScriptError {
    /// Builds a script error from a compile error, capturing the location from the source text.
    fn from_parse(path: &Path, source: &str, err: &ParseError) -> Self {
        Self {
            message: err.to_string(),
            location: SourceLocation::capture(Some(path.to_path_buf()), source, err.1),
        }
    }

    /// Builds a script error from an evaluation error in the given script file.
    ///
    /// Walks nested module errors to find the file the error actually occurred in, then re-reads
    /// that file to capture the offending line.
    fn from_eval(script_path: &Path, err: &EvalAltResult) -> Self {
        let (path, pos) = innermost_location(script_path, err);
        let location = std::fs::read_to_string(&path)
            .ok()
            .and_then(|source| SourceLocation::capture(Some(path), &source, pos));
        Self {
            message: err.to_string(),
            location,
        }
    }

    /// Builds a script error from evaluating a source string directly (no file involved).
    pub(crate) fn from_str_eval(source: &str, err: &EvalAltResult) -> Self {
        Self {
            message: err.to_string(),
            location: SourceLocation::capture(None, source, err.position()),
        }
    }
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(location) = &self.location {
            write!(f, "\n{location}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ScriptError {}

/// A location in Rhai source code, with the offending line captured eagerly.
///
/// The source line is stored at construction time, so rendering never performs IO and always
/// shows the text that actually produced the error.
#[derive(Debug, Clone)]
pub struct SourceLocation {
    /// The script file, or `None` when the source was an in-memory string.
    pub path: Option<PathBuf>,
    /// The 1-based line number.
    pub line: usize,
    /// The 1-based column number, if Rhai reported one.
    pub column: Option<usize>,
    /// The full text of the offending line.
    pub source_line: String,
}

impl SourceLocation {
    /// Captures the line at the given position from the source text.
    ///
    /// Returns `None` when the position carries no line number or lies outside the source.
    fn capture(path: Option<PathBuf>, source: &str, pos: Position) -> Option<Self> {
        let line = pos.line()?;
        let source_line = source.lines().nth(line - 1)?.to_string();
        Some(Self {
            path,
            line,
            column: pos.position(),
            source_line,
        })
    }
}

impl fmt::Display for SourceLocation {
    /// Renders a compiler-style source snippet.
    ///
    /// ```text
    ///  --> U:/gen/config/env.rhai:7:9
    ///   |
    /// 7 |         table: "logs"
    ///   |         ^
    /// ```
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let gutter = " ".repeat(self.line.to_string().len());

        if let Some(path) = &self.path {
            write!(f, "{gutter}--> {}:{}", path.display(), self.line)?;
            if let Some(column) = self.column {
                write!(f, ":{column}")?;
            }
            writeln!(f)?;
        }

        writeln!(f, "{gutter} |")?;
        write!(f, "{} | {}", self.line, self.source_line)?;

        if let Some(column) = self.column {
            // Mirror tabs from the source line so the caret stays aligned regardless of how
            // the terminal expands them.
            let padding: String = self
                .source_line
                .chars()
                .take(column.saturating_sub(1))
                .map(|c| if c == '\t' { '\t' } else { ' ' })
                .collect();
            write!(f, "\n{gutter} | {padding}^")?;
        }

        Ok(())
    }
}

/// Finds the file and position where an evaluation error actually occurred.
///
/// Errors from imported modules arrive wrapped in `ErrorInModule`, whose position points at the
/// `import` statement rather than the failure itself. This walks into the nested errors,
/// resolving each module path against its importer's directory (a no-op for the absolute paths
/// that `RelativeModuleResolver` reports).
fn innermost_location(script_path: &Path, err: &EvalAltResult) -> (PathBuf, Position) {
    if let EvalAltResult::ErrorInModule(module_path, inner, _) = err {
        let base_dir = script_path.parent().unwrap_or(Path::new("."));
        return innermost_location(&base_dir.join(module_path), inner);
    }
    (script_path.to_path_buf(), err.position())
}

/// Resolves a script's physical target while retaining the requested path on failure.
fn canonical_script_path(path: &Path) -> Result<PathBuf, RhaiError> {
    use crate::util::PathExt;

    path.canonicalize_path().map_err(|source| RhaiError::ResolveFile {
        path: path.to_path_buf(),
        source,
    })
}

// ---------------------------------------------------------------------------------------------- //

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_and_module_diagnostics_use_canonical_paths() {
        use crate::util::PathExt;

        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("modules")).unwrap();
        let main = directory.path().join("main.rhai");
        let helper = directory.path().join("helper.rhai");
        let requested = directory.path().join("modules/../main.rhai");
        std::fs::write(&main, "import \"modules/../helper.rhai\" as helper; helper::value")
            .unwrap();
        std::fs::write(&helper, "export let value = missing_function();").unwrap();
        let RhaiError::EvalScript { source, .. } = eval_script(&requested).unwrap_err() else {
            panic!("expected an imported module evaluation failure");
        };
        assert_eq!(source.location.unwrap().path, Some(helper.canonicalize_path().unwrap()));

        std::fs::write(&main, "#{ broken:").unwrap();
        let RhaiError::Compile { source, .. } = eval_script(&requested).unwrap_err() else {
            panic!("expected a main script compile failure");
        };
        assert_eq!(source.location.unwrap().path, Some(main.canonicalize_path().unwrap()));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn file_links_and_imported_links_use_their_physical_siblings() {
        let directory = tempfile::tempdir().unwrap();
        let shared = directory.path().join("shared");
        let external = directory.path().join("external");
        let local = directory.path().join("local");
        std::fs::create_dir_all(shared.join("modules")).unwrap();
        std::fs::create_dir_all(&external).unwrap();
        std::fs::create_dir_all(&local).unwrap();
        let main = shared.join("main.rhai");
        let helper = external.join("helper.rhai");
        std::fs::write(&main, "import \"modules/linked.rhai\" as helper; helper::value").unwrap();
        std::fs::write(
            &helper,
            "import \"constants.rhai\" as constants; export let value = constants::value;",
        )
        .unwrap();
        std::fs::write(external.join("constants.rhai"), "export let value = 42;").unwrap();
        let requested = local.join("config.rhai");
        if !create_file_link(&main, &requested)
            || !create_file_link(&helper, &shared.join("modules/linked.rhai"))
        {
            return;
        }

        let result = eval_script(&requested).unwrap();
        assert_eq!(result.cast::<i64>(), 42);
    }

    #[cfg(any(unix, windows))]
    fn create_file_link(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        let result = std::os::unix::fs::symlink(target, link);
        #[cfg(windows)]
        let result = std::os::windows::fs::symlink_file(target, link);
        match result {
            Ok(()) => true,
            Err(error)
                if error.kind() == std::io::ErrorKind::Unsupported
                    || (cfg!(windows) && error.raw_os_error() == Some(1314)) =>
            {
                eprintln!("skipping file-link import coverage: {error}");
                false
            }
            Err(error) => panic!("create temporary file link: {error}"),
        }
    }

    #[test]
    fn final_expression_evaluation_runs_the_script_once() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("env.rhai");
        let evaluations = Rc::new(std::cell::Cell::new(0_i64));
        let counter = evaluations.clone();
        let mut engine = Engine::new();
        engine.register_fn("next_value", move || {
            counter.set(counter.get() + 1);
            counter.get()
        });

        for script in [
            "let values = #{ value: next_value() }; values",
            "#{ value: next_value() }",
        ] {
            std::fs::write(&path, script).unwrap();
            evaluations.set(0);
            let value = RhaiEnv::new().eval(&engine, &path).unwrap().cast::<rhai::Map>();
            assert_eq!(value["value"].clone().cast::<i64>(), 1);
            assert_eq!(evaluations.get(), 1);
        }
    }

    const SOURCE: &str = "let map = #{\n    host: \"db.example.com\"\n    table: \"logs\",\n};";

    #[test]
    fn snippet_with_path_and_column() {
        let location =
            SourceLocation::capture(Some(PathBuf::from("env.rhai")), SOURCE, Position::new(3, 5))
                .unwrap();
        let expected = " --> env.rhai:3:5\n  |\n3 |     table: \"logs\",\n  |     ^";
        assert_eq!(location.to_string(), expected);
    }

    #[test]
    fn snippet_without_path() {
        let location = SourceLocation::capture(None, SOURCE, Position::new(3, 5)).unwrap();
        let expected = "  |\n3 |     table: \"logs\",\n  |     ^";
        assert_eq!(location.to_string(), expected);
    }

    #[test]
    fn snippet_caret_mirrors_tabs() {
        let location =
            SourceLocation::capture(None, "\t\tlet x = ;", Position::new(1, 11)).unwrap();
        let expected = "  |\n1 | \t\tlet x = ;\n  | \t\t        ^";
        assert_eq!(location.to_string(), expected);
    }

    #[test]
    fn snippet_gutter_matches_line_number_width() {
        let source = "ok\n".repeat(11) + "bad";
        let location = SourceLocation::capture(None, &source, Position::new(12, 1)).unwrap();
        let expected = "   |\n12 | bad\n   | ^";
        assert_eq!(location.to_string(), expected);
    }

    #[test]
    fn capture_returns_none_without_line() {
        assert!(SourceLocation::capture(None, SOURCE, Position::NONE).is_none());
    }

    #[test]
    fn capture_returns_none_for_line_out_of_range() {
        assert!(SourceLocation::capture(None, SOURCE, Position::new(99, 1)).is_none());
    }

    #[test]
    fn innermost_location_unwraps_nested_module_errors() {
        let leaf = EvalAltResult::ErrorRuntime(Dynamic::from("boom"), Position::new(7, 9));
        let middle = EvalAltResult::ErrorInModule(
            "sub/inner.rhai".to_string(),
            Box::new(leaf),
            Position::new(2, 1),
        );
        let outer = EvalAltResult::ErrorInModule(
            "env.rhai".to_string(),
            Box::new(middle),
            Position::new(1, 8),
        );

        let (path, pos) = innermost_location(Path::new("cfg/generate.rhai"), &outer);
        assert_eq!(path, Path::new("cfg").join("sub/inner.rhai"));
        assert_eq!((pos.line(), pos.position()), (Some(7), Some(9)));
    }

    #[test]
    fn eval_str_error_carries_location() {
        let err = eval_str("#{ a: 1\nb: 2 }").unwrap_err();
        let RhaiError::Eval { source } = err else {
            panic!("expected RhaiError::Eval");
        };
        let location = source.location.expect("location should be captured");
        assert_eq!(location.line, 2);
        assert_eq!(location.source_line, "b: 2 }");
    }
}
