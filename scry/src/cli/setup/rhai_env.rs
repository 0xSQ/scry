//! Loads the optional app environment used by Rhai configuration scripts.

use std::path::Path;

use rhai::Dynamic;

use super::{find_config_with_registry, ApplicationConfig, FindConfigError};
use crate::node::{Format, FormatParserRegistry};
use crate::rhai::{eval_script, RhaiEnv};
use crate::util::PathExt;
use crate::{BoxedError, NodeError};

// ---------------------------------------------------------------------------------------------- //

/// Loads an application's optional `env.*` config as the Rhai constant `env`.
///
/// Resolves an optional `root.*` once, then discovers only `env.*` in the effective root.
/// If no file is found, returns an environment without an `env` constant. Discovery and
/// loading errors are propagated. Application-wide command maps never supply the environment.
///
/// A Rhai file supplies its final returned value. It is evaluated once with a plain engine and
/// relative imports, without injected constants or application functions. Exported variables do
/// not select the environment value. Rhai values are preserved directly. Other formats supply
/// their root map. Local and shared environments are not merged.
///
/// The application evaluates its configs with the returned environment and installs
/// [`RhaiEnv::module_resolver`] on its engine to inject the same constant into imported modules.
pub fn load_app_rhai_env(app: &str) -> Result<RhaiEnv, BoxedError> {
    Ok(ApplicationConfig::new(app).load_environment()?)
}

pub(super) fn load_rhai_env_in(
    directory: &Path,
    registry: &FormatParserRegistry,
    file_stem: &str,
    binding: &str,
) -> Result<RhaiEnv, BoxedError> {
    let path = match find_config_with_registry(directory, file_stem, registry) {
        Ok(path) => path,
        Err(FindConfigError::NotFound { .. }) => return Ok(RhaiEnv::new()),
        Err(error) => return Err(error.into()),
    };
    load_rhai_env_with_registry(&path, registry, binding)
}

fn load_rhai_env_with_registry(
    path: &Path,
    registry: &FormatParserRegistry,
    binding: &str,
) -> Result<RhaiEnv, BoxedError> {
    let value = load_env_value(path, registry).map_err(|source| {
        NodeError::with_context_boxed(
            format!("Failed to load environment config '{}'", path.display()),
            source,
        )
    })?;
    let mut env = RhaiEnv::new();
    env.add_constant(binding, value);
    Ok(env)
}

fn load_env_value(path: &Path, registry: &FormatParserRegistry) -> Result<Dynamic, BoxedError> {
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    let parser = registry.parser_for_extension(extension).ok_or_else(|| {
        crate::node::FormatError::unknown_file_extension(
            path,
            extension,
            &registry.supported_extensions(),
        )
    })?;
    let physical = path.canonicalize_path()?;
    if parser.id() == Format::Rhai {
        return Ok(eval_script(&physical)?);
    }

    let node = parser.parse_file(&physical)?;
    node.as_map()?;
    Ok(node.to_rhai_dynamic())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::default_format_parser_registry;
    use indoc::indoc;
    use rhai::Engine;
    use tempfile::tempdir;

    #[test]
    fn an_absent_environment_leaves_the_constant_undefined() {
        let directory = tempdir().unwrap();
        let app = directory.path().file_name().unwrap().to_str().unwrap();
        assert!(!load_app_rhai_env(app).unwrap().scope().contains("env"));
    }

    #[test]
    fn rhai_uses_the_returned_value_and_preserves_raw_values() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("env.rhai");
        std::fs::write(
            &path,
            indoc! {r#"
                fn callback() { 1 }
                export let env = #{ source: "binding" };
                #{ source: "returned", callback: Fn("callback") }
            "#},
        )
        .unwrap();
        let env = load_rhai_env_from_path(&path).unwrap();
        let value = env.scope().get_value::<rhai::Map>("env").unwrap();
        assert_eq!(value["source"].clone().cast::<String>(), "returned");
        assert!(value["callback"].is::<rhai::FnPtr>());

        std::fs::write(
            &path,
            "fn callback() { 1 } export let env = #{ source: \"binding\" }; Fn(\"callback\")",
        )
        .unwrap();
        let env = load_rhai_env_from_path(&path).unwrap();
        assert!(env.scope().get_value::<rhai::FnPtr>("env").is_some());

        std::fs::write(&path, "export let env = #{ source: \"binding\" };").unwrap();
        let env = load_rhai_env_from_path(&path).unwrap();
        assert!(env.scope().get_value::<Dynamic>("env").unwrap().is_unit());
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
    fn custom_file_stem_and_binding_reach_configs_and_imports_without_a_default_env() {
        let directory = tempdir().unwrap();
        std::fs::write(
            directory.path().join("defaults.rhai"),
            "export let config = #{ output_dir: \"custom-output\" };",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("settings.rhai"),
            indoc! {r#"
                import "defaults.rhai" as defaults;
                export let env = #{ output_dir: "wrong-env-binding" };
                export let machine = #{ output_dir: "wrong-machine-binding" };
                defaults::config
            "#},
        )
        .unwrap();
        std::fs::write(
            directory.path().join("env.rhai"),
            "#{ output_dir: \"wrong-default-file\" }",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("values.rhai"),
            "export let output_dir = machine.output_dir;",
        )
        .unwrap();
        let config_path = directory.path().join("config.rhai");
        std::fs::write(
            &config_path,
            "import \"values.rhai\" as values; #{ direct: machine.output_dir, imported: values::output_dir }",
        )
        .unwrap();
        let registry = default_format_parser_registry();
        let env = load_rhai_env_in(directory.path(), &registry, "settings", "machine").unwrap();
        assert!(!env.scope().contains("env"));
        let mut engine = Engine::new();
        engine.set_module_resolver(env.module_resolver());
        let value = env.eval(&engine, &config_path).unwrap().cast::<rhai::Map>();
        assert_eq!(value["direct"].clone().cast::<String>(), "custom-output");
        assert_eq!(value["imported"].clone().cast::<String>(), "custom-output");

        let absent = load_rhai_env_in(directory.path(), &registry, "absent", "machine").unwrap();
        assert!(!absent.scope().contains("machine"));
        assert!(!absent.scope().contains("env"));
    }

    #[test]
    fn custom_bootstrap_cannot_use_its_injected_binding_or_application_functions() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("settings.rhai");
        let registry = default_format_parser_registry();
        for script in [
            "#{ copied: machine.output_dir }",
            "#{ copied: env.output_dir }",
            "#{ values: app::values() }",
        ] {
            std::fs::write(&path, script).unwrap();
            let error =
                load_rhai_env_in(directory.path(), &registry, "settings", "machine").err().unwrap();
            assert!(error.to_string().contains("settings.rhai"));
            assert!(error.source().is_some());
        }
        std::fs::write(
            directory.path().join("defaults.rhai"),
            "export let value = machine.output_dir;",
        )
        .unwrap();
        std::fs::write(&path, "import \"defaults.rhai\" as defaults; defaults::value").unwrap();
        assert!(load_rhai_env_in(directory.path(), &registry, "settings", "machine").is_err());
    }

    #[cfg(feature = "format-json")]
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

    #[cfg(feature = "format-json")]
    #[test]
    fn renamed_json_environment_keeps_its_whole_map_under_the_custom_binding() {
        let directory = tempdir().unwrap();
        std::fs::write(
            directory.path().join("settings.json"),
            r#"{ "machine": { "nested": true }, "source": "returned-root" }"#,
        )
        .unwrap();
        let env = load_rhai_env_in(
            directory.path(),
            &default_format_parser_registry(),
            "settings",
            "machine",
        )
        .unwrap();
        assert!(!env.scope().contains("env"));
        let value = env.scope().get_value::<rhai::Map>("machine").unwrap();
        assert_eq!(value["source"].clone().cast::<String>(), "returned-root");
        assert!(value["machine"].is::<rhai::Map>());
    }

    #[test]
    fn malformed_files_do_not_fall_back_to_an_empty_environment() {
        let directory = tempdir().unwrap();
        let mut names = vec!["env.rhai"];
        if cfg!(feature = "format-json") {
            names.push("env.json");
        }
        for name in names {
            let path = directory.path().join(name);
            std::fs::write(&path, "#{ broken: ").unwrap();
            assert!(load_rhai_env_from_path(&path).is_err());
        }
    }

    fn load_rhai_env_from_path(path: &Path) -> Result<RhaiEnv, BoxedError> {
        load_rhai_env_with_registry(path, &default_format_parser_registry(), "env")
    }
}
