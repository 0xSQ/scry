//! App-level redirect files for command config discovery.
//!
//! A redirect file replaces per-command "trampoline" configs under `~/.config/<app>/` with a
//! single app-named file that points command config discovery elsewhere. The file simply *is*
//! the redirect value - the final expression for Rhai (or a conventional `config` export), the
//! document root for other formats. A string is shorthand for the default directory:
//!
//! ```rhai
//! // ~/.config/myapp/myapp.rhai - the entire file:
//! "~/dev/myapp-configs"
//! ```
//!
//! Full form for exceptions:
//!
//! ```rhai
//! #{
//!     dir: "~/dev/myapp-configs",
//!     commands: #{
//!         convert: "~/dev/experiments/convert-alt.rhai",
//!         // Group redirect: all `backup` subcommands load from this one file.
//!         backup: "~/dev/myapp-configs/backup.rhai",
//!     },
//! }
//! ```
//!
//! Resolution order for a command (implemented by [`resolve_command_config`]):
//! 1. `~/.config/<app>/<command>.*` - a per-command file always wins.
//! 2. The redirect file's `commands` entry for the command path (most specific match first).
//!    An entry pointing at a missing file is an error, not a fallthrough.
//! 3. `<command>.*` inside the redirect file's `dir`.
//!
//! Relative paths in the redirect file resolve against the redirect file's directory; a leading
//! `~/` resolves against the home directory.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use super::config_source::{find_config_default, home_config_dir, FindConfigError};
use crate::desc::Desc;
use crate::node::{Node, NodeError};
use crate::rhai::{eval_script, eval_script_var, RhaiError};
use crate::util::PathExt;
use crate::{BoxedError, Describe, FromNode};

// ---------------------------------------------------------------------------------------------- //

/// Resolves a command's config path through the redirect machinery.
///
/// Implements the resolution order described in the module docs. Returns `Ok(None)` when nothing
/// was found anywhere, so callers can fall through to a missing-config policy. Use
/// [`require_command_config`] to turn that case into an error narrating the attempted steps.
///
/// `command_path` is the full command path, e.g. `&["convert"]` or `&["backup", "create"]`.
pub fn resolve_command_config(
    app: &str,
    command_path: &[&str],
) -> Result<Option<PathBuf>, RedirectError> {
    let config_dir = home_config_dir(app).ok_or(RedirectError::NoHomeDir)?;
    Ok(resolve_in_dir(&config_dir, app, command_path)?.path)
}

/// Resolves a command's config path, erroring when nothing is found.
///
/// Like [`resolve_command_config`], but a fully exhausted resolution chain becomes a
/// [`RedirectError::NotFound`] that lists every step that was tried.
pub fn require_command_config(app: &str, command_path: &[&str]) -> Result<PathBuf, RedirectError> {
    let config_dir = home_config_dir(app).ok_or(RedirectError::NoHomeDir)?;
    let resolution = resolve_in_dir(&config_dir, app, command_path)?;
    match resolution.path {
        Some(path) => Ok(path),
        None => Err(RedirectError::NotFound {
            command: command_path.join(" "),
            tried: resolution.tried,
        }),
    }
}

// ---------------------------------------------------------------------------------------------- //

/// Parsed contents of a redirect file.
///
/// A plain string is shorthand for `{ dir: <string> }`.
#[derive(Debug, Clone, Default)]
pub struct RedirectSpec {
    /// Default directory to discover `<command>.<ext>` files in.
    pub dir: Option<String>,
    /// Explicit per-command target files. Keys are dotted command paths (e.g. `"meta.inspect"`);
    /// a group key (e.g. `"meta"`) redirects all its subcommands.
    pub commands: IndexMap<String, String>,
}

impl FromNode for RedirectSpec {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        // String shorthand: the default directory.
        if node.kind.is_leaf() {
            let dir: String = node.as_type()?;
            return Ok(RedirectSpec {
                dir: Some(dir),
                commands: IndexMap::new(),
            });
        }

        if node.kind.is_map() {
            let dir: Option<String> = node.opt("dir")?;
            let mut commands = IndexMap::new();
            if let Some(commands_node) = node.opt_node("commands")? {
                for (key, value) in commands_node.as_map()? {
                    commands.insert(key.clone(), value.as_type::<String>()?);
                }
            }
            node.ensure_no_unknown_keys()?;
            return Ok(RedirectSpec { dir, commands });
        }

        Err(NodeError::invalid_value(
            &node.path,
            "expected string or map with 'dir'/'commands' keys",
        ))
    }
}

impl Describe for RedirectSpec {
    fn describe() -> Desc {
        Desc::plain("config redirect specification")
    }
}

// ---------------------------------------------------------------------------------------------- //
// Resolution

/// The outcome of one resolution run: the found path, plus a trail of failed steps for errors.
#[derive(Debug)]
struct Resolution {
    path: Option<PathBuf>,
    tried: Vec<String>,
}

/// Runs the resolution chain against an explicit config directory.
fn resolve_in_dir(
    config_dir: &Path,
    app: &str,
    command_path: &[&str],
) -> Result<Resolution, RedirectError> {
    let mut tried = Vec::new();
    let command_name = command_path.join(".");

    // A per-command file always wins.
    match find_config_default(config_dir, &command_name) {
        Ok(path) => {
            return Ok(Resolution {
                path: Some(path),
                tried,
            });
        }
        Err(err @ FindConfigError::Ambiguous { .. }) => return Err(err.into()),
        Err(FindConfigError::NotFound { .. }) => {
            tried.push(format!("no '{command_name}.*' in '{}'", config_dir.display()));
        }
    }

    // Consult the app-named redirect file.
    let redirect_path = match find_config_default(config_dir, app) {
        Ok(path) => path,
        Err(err @ FindConfigError::Ambiguous { .. }) => return Err(err.into()),
        Err(FindConfigError::NotFound { .. }) => {
            tried.push(format!("no '{app}.*' redirect file in '{}'", config_dir.display()));
            return Ok(Resolution { path: None, tried });
        }
    };
    let spec = load_redirect_spec(&redirect_path)?;
    let redirect_dir = redirect_path.parent().unwrap_or(Path::new(".")).to_path_buf();

    // Explicit command entries, most specific first.
    for len in (1..=command_path.len()).rev() {
        let key = command_path[..len].join(".");
        if let Some(target) = spec.commands.get(&key) {
            let target_path = resolve_redirect_path(&redirect_dir, target);
            if !target_path.is_file() {
                return Err(RedirectError::MissingTarget {
                    redirect_file: redirect_path,
                    entry: key,
                    target: target_path,
                });
            }
            return Ok(Resolution {
                path: Some(target_path),
                tried,
            });
        }
    }
    tried.push(format!(
        "redirect file '{}' has no entry for '{command_name}'",
        redirect_path.display()
    ));

    // Fall back to the redirect file's default directory.
    let Some(dir) = &spec.dir else {
        tried.push(format!("redirect file '{}' declares no 'dir'", redirect_path.display()));
        return Ok(Resolution { path: None, tried });
    };
    let dir = resolve_redirect_path(&redirect_dir, dir);
    match find_config_default(&dir, &command_name) {
        Ok(path) => Ok(Resolution {
            path: Some(path),
            tried,
        }),
        Err(err @ FindConfigError::Ambiguous { .. }) => Err(err.into()),
        Err(FindConfigError::NotFound { .. }) => {
            tried.push(format!("no '{command_name}.*' in redirect dir '{}'", dir.display()));
            Ok(Resolution { path: None, tried })
        }
    }
}

/// Loads and parses a redirect file into a spec.
fn load_redirect_spec(path: &Path) -> Result<RedirectSpec, RedirectError> {
    let node = load_redirect_node(path).map_err(|source| RedirectError::LoadRedirectFile {
        path: path.to_path_buf(),
        source,
    })?;
    node.as_type::<RedirectSpec>().map_err(|source| RedirectError::InvalidRedirectFile {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

/// Loads the redirect file's value: a `config` export or the final expression for Rhai, the
/// document root for other formats.
fn load_redirect_node(path: &Path) -> Result<Node, BoxedError> {
    let ext = path.ext_str()?.to_ascii_lowercase();
    if ext != "rhai" {
        return Ok(Node::parse_file(path)?);
    }

    match eval_script_var(path, "config") {
        Ok(dynamic) => return Ok(Node::from_rhai_dynamic(dynamic)?),
        Err(RhaiError::VarNotFound { .. }) => {}
        Err(err) => return Err(err.into()),
    }
    Ok(Node::from_rhai_dynamic(eval_script(path)?)?)
}

/// Resolves a path value from a redirect file: `~/` expands to the home directory, relative
/// paths resolve against the redirect file's directory, absolute paths stand as-is.
fn resolve_redirect_path(redirect_dir: &Path, value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/").or_else(|| value.strip_prefix("~\\")) {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    redirect_dir.join(value)
}

// ---------------------------------------------------------------------------------------------- //
// RedirectError

/// Errors from resolving a command config through a redirect file.
#[derive(Debug, thiserror::Error)]
pub enum RedirectError {
    /// Could not determine the home directory.
    #[error("could not determine home directory")]
    NoHomeDir,

    /// A config file lookup was ambiguous.
    #[error(transparent)]
    Find(#[from] FindConfigError),

    /// Failed to load the redirect file.
    #[error("failed to load redirect file '{}'", path.display())]
    LoadRedirectFile {
        path: PathBuf,
        #[source]
        source: BoxedError,
    },

    /// The redirect file did not evaluate to valid redirect data.
    #[error("invalid redirect file '{}'", path.display())]
    InvalidRedirectFile {
        path: PathBuf,
        #[source]
        source: Box<NodeError>,
    },

    /// An explicit `commands` entry points at a file that does not exist.
    #[error(
        "redirect entry '{entry}' in '{}' points to missing file '{}'",
        redirect_file.display(),
        target.display()
    )]
    MissingTarget {
        redirect_file: PathBuf,
        entry: String,
        target: PathBuf,
    },

    /// The whole resolution chain came up empty.
    #[error("no config file found for command '{command}'{}", format_tried(tried))]
    NotFound { command: String, tried: Vec<String> },
}

/// Formats the trail of failed resolution steps as indented list lines.
fn format_tried(tried: &[String]) -> String {
    let mut out = String::new();
    for step in tried {
        out.push_str("\n  - ");
        out.push_str(step);
    }
    out
}

// ---------------------------------------------------------------------------------------------- //
// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn per_command_file_wins_over_redirect() {
        let temp = TempDir::new().unwrap();
        let expected = write(temp.path(), "conjure.rhai", "#{}");
        write(temp.path(), "app.rhai", r#""somewhere/else""#);

        let resolution = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap();
        assert_eq!(resolution.path, Some(expected));
    }

    #[test]
    fn string_shorthand_redirects_to_dir() {
        let temp = TempDir::new().unwrap();
        let real_dir = temp.path().join("real");
        fs::create_dir(&real_dir).unwrap();
        let expected = write(&real_dir, "conjure.rhai", "#{}");
        write(temp.path(), "app.rhai", r#""real""#);

        let resolution = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap();
        assert_eq!(resolution.path, Some(expected));
    }

    #[test]
    fn config_export_works_as_redirect_value() {
        let temp = TempDir::new().unwrap();
        let real_dir = temp.path().join("real");
        fs::create_dir(&real_dir).unwrap();
        let expected = write(&real_dir, "generate.rhai", "#{}");
        write(temp.path(), "app.rhai", r#"export let config = "real";"#);

        let resolution = resolve_in_dir(temp.path(), "app", &["generate"]).unwrap();
        assert_eq!(resolution.path, Some(expected));
    }

    #[test]
    fn explicit_entry_wins_over_dir() {
        let temp = TempDir::new().unwrap();
        let real_dir = temp.path().join("real");
        fs::create_dir(&real_dir).unwrap();
        write(&real_dir, "conjure.rhai", "#{}");
        let special = write(temp.path(), "special.rhai", "#{}");
        write(
            temp.path(),
            "app.rhai",
            r#"#{ dir: "real", commands: #{ conjure: "special.rhai" } }"#,
        );

        let resolution = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap();
        assert_eq!(resolution.path, Some(special));
    }

    #[test]
    fn most_specific_command_entry_wins() {
        let temp = TempDir::new().unwrap();
        let group = write(temp.path(), "group.rhai", "#{}");
        let specific = write(temp.path(), "specific.rhai", "#{}");
        write(
            temp.path(),
            "app.rhai",
            r#"#{ commands: #{ meta: "group.rhai", "meta.inspect": "specific.rhai" } }"#,
        );

        let resolution = resolve_in_dir(temp.path(), "app", &["meta", "inspect"]).unwrap();
        assert_eq!(resolution.path, Some(specific));

        let resolution = resolve_in_dir(temp.path(), "app", &["meta", "comfy"]).unwrap();
        assert_eq!(resolution.path, Some(group));
    }

    #[test]
    fn missing_entry_target_is_an_error() {
        let temp = TempDir::new().unwrap();
        write(temp.path(), "app.rhai", r#"#{ commands: #{ conjure: "nowhere.rhai" } }"#);

        let err = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap_err();
        assert!(matches!(err, RedirectError::MissingTarget { .. }));
    }

    #[test]
    fn exhausted_chain_returns_none_with_trail() {
        let temp = TempDir::new().unwrap();
        write(temp.path(), "app.rhai", r#"#{ commands: #{ other: "app.rhai" } }"#);

        let resolution = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap();
        assert_eq!(resolution.path, None);
        assert_eq!(resolution.tried.len(), 3);
        assert!(resolution.tried[1].contains("no entry for 'conjure'"));
        assert!(resolution.tried[2].contains("declares no 'dir'"));
    }

    #[test]
    fn no_redirect_file_returns_none() {
        let temp = TempDir::new().unwrap();

        let resolution = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap();
        assert_eq!(resolution.path, None);
        assert_eq!(resolution.tried.len(), 2);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let temp = TempDir::new().unwrap();
        write(temp.path(), "app.rhai", r#"#{ dir: "real", commandz: "oops" }"#);

        let err = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap_err();
        assert!(matches!(err, RedirectError::InvalidRedirectFile { .. }));
    }

    #[test]
    fn json_redirect_file_works() {
        let temp = TempDir::new().unwrap();
        let real_dir = temp.path().join("real");
        fs::create_dir(&real_dir).unwrap();
        let expected = write(&real_dir, "conjure.json", r#"{"conjure": {}}"#);
        write(temp.path(), "app.json", r#""real""#);

        let resolution = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap();
        assert_eq!(resolution.path, Some(expected));
    }

    #[test]
    fn tilde_paths_resolve_against_home() {
        let home = dirs::home_dir().unwrap();
        let resolved = resolve_redirect_path(Path::new("base"), "~/configs");
        assert_eq!(resolved, home.join("configs"));
    }
}
