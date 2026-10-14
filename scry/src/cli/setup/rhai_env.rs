//! Loads the optional app environment used by Rhai configuration scripts.

use std::path::Path;

use rhai::{Dynamic, Engine};

use super::resolve_command_config;
use crate::rhai::RhaiEnv;
use crate::util::PathExt;
use crate::{BoxedError, Node, NodeError};

// ---------------------------------------------------------------------------------------------- //

/// Loads an application's optional `env.*` config as the Rhai constant `env`.
///
/// Uses [`resolve_command_config`] with the name `env`, so local files take precedence over
/// app-level command routes and shared directories. If no file is found, returns an environment
/// without an `env` constant. Discovery and loading errors are propagated.
///
/// A Rhai file supplies its `env` binding when present, otherwise its final expression. It is
/// evaluated once with a plain engine and relative imports, without injected constants or
/// application functions. Rhai values are preserved directly. Other formats supply their root
/// map. Local and shared environments are not merged.
///
/// The application evaluates its configs with the returned environment and installs
/// [`RhaiEnv::module_resolver`] on its engine to inject the same constant into imported modules.
pub fn load_app_rhai_env(app: &str) -> Result<RhaiEnv, BoxedError> {
    match resolve_command_config(app, &["env"])? {
        Some(path) => load_rhai_env_from_path(&path),
        None => Ok(RhaiEnv::new()),
    }
}

fn load_rhai_env_from_path(path: &Path) -> Result<RhaiEnv, BoxedError> {
    let value = load_env_value(path).map_err(|source| {
        NodeError::with_context_boxed(
            format!("Failed to load env config '{}'", path.display()),
            source,
        )
    })?;
    let mut env = RhaiEnv::new();
    env.add_constant("env", value);
    Ok(env)
}

fn load_env_value(path: &Path) -> Result<Dynamic, BoxedError> {
    if path.ext_str()?.eq_ignore_ascii_case("rhai") {
        let env = RhaiEnv::new();
        let mut engine = Engine::new();
        engine.set_module_resolver(env.module_resolver());
        return Ok(env.eval_var_or_value(&engine, path, "env")?);
    }

    let node = Node::parse_file(path)?;
    node.as_map()?;
    Ok(node.to_rhai_dynamic())
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use tempfile::tempdir;

    #[test]
    fn an_absent_environment_leaves_the_constant_undefined() {
        let directory = tempdir().unwrap();
        let app = directory.path().file_name().unwrap().to_str().unwrap();
        assert!(!load_app_rhai_env(app).unwrap().scope().contains("env"));
    }

    #[test]
    fn rhai_prefers_the_binding_and_preserves_raw_values() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("env.rhai");
        std::fs::write(
            &path,
            indoc! {r#"
                fn callback() { 1 }
                export let env = #{ source: "binding", callback: Fn("callback") };
                Fn("callback")
            "#},
        )
        .unwrap();
        let env = load_rhai_env_from_path(&path).unwrap();
        let value = env.scope().get_value::<rhai::Map>("env").unwrap();
        assert_eq!(value["source"].clone().cast::<String>(), "binding");
        assert!(value["callback"].is::<rhai::FnPtr>());
    }

    #[test]
    fn bootstrap_imports_are_relative_and_configs_and_imports_receive_env() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("env.rhai");
        std::fs::write(
            directory.path().join("defaults.rhai"),
            "export let config = #{ output_dir: \"machine-output\" };",
        )
        .unwrap();
        std::fs::write(&path, "import \"defaults.rhai\" as defaults; defaults::config").unwrap();
        std::fs::write(
            directory.path().join("values.rhai"),
            "export let output_dir = env.output_dir;",
        )
        .unwrap();
        let config_path = directory.path().join("config.rhai");
        std::fs::write(
            &config_path,
            "import \"values.rhai\" as values; #{ direct: env.output_dir, imported: values::output_dir }",
        )
        .unwrap();
        let env = load_rhai_env_from_path(&path).unwrap();
        let mut engine = Engine::new();
        engine.set_module_resolver(env.module_resolver());
        let config = env.eval(&engine, &config_path).unwrap().cast::<rhai::Map>();
        assert_eq!(config["direct"].clone().cast::<String>(), "machine-output");
        assert_eq!(config["imported"].clone().cast::<String>(), "machine-output");
    }

    #[test]
    fn bootstrap_cannot_use_env_or_application_functions() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("env.rhai");
        for script in ["#{ copied: env.output_dir }", "#{ values: app::values() }"] {
            std::fs::write(&path, script).unwrap();
            let error = load_rhai_env_from_path(&path).err().unwrap();
            assert!(error.to_string().contains("env.rhai"));
            assert!(error.source().is_some());
        }
    }

    #[test]
    fn json_keeps_the_whole_root_map_and_rejects_other_roots() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("env.json");
        std::fs::write(&path, r#"{ "env": { "value": 1 }, "machine": "local" }"#).unwrap();
        let env = load_rhai_env_from_path(&path).unwrap();
        let value = env.scope().get_value::<rhai::Map>("env").unwrap();
        assert_eq!(value["machine"].clone().cast::<String>(), "local");
        assert!(value["env"].is::<rhai::Map>());

        std::fs::write(&path, "[1, 2]").unwrap();
        assert!(load_rhai_env_from_path(&path).is_err());
    }

    #[test]
    fn malformed_files_do_not_fall_back_to_an_empty_environment() {
        let directory = tempdir().unwrap();
        for name in ["env.rhai", "env.json"] {
            let path = directory.path().join(name);
            std::fs::write(&path, "#{ broken: ").unwrap();
            assert!(load_rhai_env_from_path(&path).is_err());
        }
    }
}
