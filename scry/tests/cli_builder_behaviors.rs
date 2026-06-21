//! Integration tests for CLI Setup behaviors.
//!
//! These tests document and lock down the current behavior of the Setup. They cover:
//! - Config path resolution (positional, flag, both, auto-discover)
//! - Query args (--desc, --get, --get-flat, --get-as)
//! - Override args (--set, --remove)
//! - Expose args (flags and options)
//! - Collision detection
#![cfg(feature = "format-json")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use scry::cli::setup::{
    ConfigSource, ExposeMap, MissingConfigPolicy, MultiSourcePolicy, OverrideArgs, QueryArgs,
    Required, Setup, SetupError,
};
use scry::node::Format;
use scry::{Config, Node};

// ---------------------------------------------------------------------------------------------- //
// Test Config Types

#[derive(Debug, Config)]
struct SimpleConfig {
    /// A string value.
    value: String,
}

#[derive(Debug, Config)]
#[allow(dead_code)]
struct NestedConfig {
    /// Top-level name.
    name: String,
    /// Database settings.
    database: DatabaseConfig,
    /// Enable verbose mode.
    #[scry(default = false)]
    verbose: bool,
}

#[derive(Debug, Config)]
struct DatabaseConfig {
    /// Database host.
    host: String,
    /// Database port.
    #[scry(default = 5432)]
    port: u16,
}

#[derive(Debug, Config)]
#[allow(dead_code)]
struct ListTestConfig {
    /// Items list.
    #[scry(default)]
    items: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, scry::FromNode, scry::ToNode, scry::Describe)]
enum ProcessOrder {
    /// Traverses rows first.
    RowMajor,
    /// Traverses columns first.
    ColMajor,
}

#[derive(Debug, Config)]
struct EnumExposeConfig {
    /// Chooses the grid traversal order.
    order: ProcessOrder,
}

// ---------------------------------------------------------------------------------------------- //
// Test Helpers

fn json_loader(json: &'static str) -> impl Fn(&Path) -> Result<Node, std::convert::Infallible> {
    move |_| Ok(Node::parse_str(json, Format::Json).unwrap())
}

fn simple_config_loader() -> impl Fn(&Path) -> Result<Node, std::convert::Infallible> {
    json_loader(r#"{"value": "test"}"#)
}

fn nested_config_loader() -> impl Fn(&Path) -> Result<Node, std::convert::Infallible> {
    json_loader(
        r#"{"name": "myapp", "database": {"host": "localhost", "port": 5432}, "verbose": false}"#,
    )
}

// ---------------------------------------------------------------------------------------------- //
// Config Path Resolution Tests

mod config_path_resolution {
    use super::*;

    #[test]
    fn positional_only() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::standard())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(simple_config_loader())
            })
            .into_bundle(move |cfg: SimpleConfig| {
                assert_eq!(cfg.value, "test");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from(["test", "config.json"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn flag_only() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        // Start fresh ConfigSource to get only a flag, no positional.
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::standard())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .option("config", None, "Path to config file")
                    .loader(simple_config_loader())
            })
            .into_bundle(move |cfg: SimpleConfig| {
                assert_eq!(cfg.value, "test");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from(["test", "--config", "config.json"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn flag_with_short() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        // Start fresh ConfigSource to get only a flag, no positional.
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .option("config", Some('c'), "Path to config file")
                    .loader(simple_config_loader())
            })
            .into_bundle(move |cfg: SimpleConfig| {
                assert_eq!(cfg.value, "test");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from(["test", "-c", "config.json"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn both_positional_and_flag_uses_positional_first() {
        // Current behavior: positional is checked first, then flag
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file", Required::No)
                    .option("config", None, "Path to config file")
                    .multi_source_policy(MultiSourcePolicy::PreferPositional)
                    .loader(simple_config_loader())
            })
            .into_bundle(move |cfg: SimpleConfig| {
                assert_eq!(cfg.value, "test");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        // Provide positional
        let result = bundle.run_from(["test", "config.json"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn both_positional_and_flag_uses_flag_when_no_positional() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file", Required::No)
                    .option("config", None, "Path to config file")
                    .multi_source_policy(MultiSourcePolicy::PreferPositional)
                    .loader(simple_config_loader())
            })
            .into_bundle(move |cfg: SimpleConfig| {
                assert_eq!(cfg.value, "test");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        // Provide flag only
        let result = bundle.run_from(["test", "--config", "config.json"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn error_on_multiple_rejects_both_sources() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file", Required::No)
                    .option("config", None, "Path to config file")
                    .multi_source_policy(MultiSourcePolicy::ErrorOnMultiple)
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});

        // Providing both positional and flag should error
        let result = bundle.run_from(["test", "config.json", "--config", "other.json"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, SetupError::MultipleConfigPaths),
            "Expected SetupError::MultipleConfigPaths, got {:?}",
            err
        );
    }

    #[test]
    fn prefer_option_policy_uses_option_over_positional() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        // Create a loader that returns different configs based on path.
        let loader = move |path: &Path| -> Result<Node, std::convert::Infallible> {
            let value = if path.to_str().unwrap().contains("from_option") {
                "from_option"
            } else {
                "from_positional"
            };
            Ok(Node::parse_str(&format!(r#"{{"value": "{}"}}"#, value), Format::Json).unwrap())
        };

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file", Required::No)
                    .option("config", None, "Path to config file")
                    .multi_source_policy(MultiSourcePolicy::PreferOption)
                    .loader(loader)
            })
            .into_bundle(move |cfg: SimpleConfig| {
                assert_eq!(cfg.value, "from_option");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        // Providing both should use option (PreferOption policy)
        let result = bundle.run_from(["test", "positional.json", "--config", "from_option.json"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn discovery_function_resolves_config() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file", Required::No)
                    .discover(|_cmd| {
                        Ok::<_, std::convert::Infallible>(Some(PathBuf::from("discovered.json")))
                    })
                    .loader(simple_config_loader())
            })
            .into_bundle(move |cfg: SimpleConfig| {
                assert_eq!(cfg.value, "test");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        // No positional provided, should use discovery.
        let result = bundle.run_from(["test"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn discovery_error_is_cli_error() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file", Required::No)
                    .discover(|_cmd| Err::<Option<PathBuf>, _>(anyhow::anyhow!("discovery failed")))
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});

        let result = bundle.run_from(["test"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, SetupError::ConfigNotFound { .. }),
            "Expected SetupError::ConfigNotFound, got {:?}",
            err
        );
    }

    #[test]
    fn no_config_source_is_error() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file", Required::No)
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});

        let result = bundle.run_from(["test"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, SetupError::NoConfigFile { .. }),
            "Expected SetupError::NoConfigFile, got {:?}",
            err
        );
    }

    #[test]
    fn no_config_source_can_start_from_empty_node() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::standard())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file", Required::No)
                    .or_empty()
            })
            .into_bundle(move |cfg: SimpleConfig| {
                assert_eq!(cfg.value, "from_cli");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from(["test", "--set", "value", "from_cli"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn empty_discovery_falls_through_to_empty_node_policy() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::standard())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file", Required::No)
                    .discover(|_cmd| Ok::<_, std::convert::Infallible>(None))
                    .or_empty()
            })
            .into_bundle(move |cfg: SimpleConfig| {
                assert_eq!(cfg.value, "from_cli");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from(["test", "--set", "value", "from_cli"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn empty_discovery_without_empty_node_policy_is_error() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file", Required::No)
                    .discover(|_cmd| Ok::<_, std::convert::Infallible>(None))
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});

        let result = bundle.run_from(["test"]);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), SetupError::NoConfigFile { .. }),
            "Expected SetupError::NoConfigFile"
        );
    }

    #[test]
    fn discovery_takes_precedence_over_empty_node_policy() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file", Required::No)
                    .missing_policy(MissingConfigPolicy::EmptyNode)
                    .discover(|_cmd| {
                        Ok::<_, std::convert::Infallible>(Some(PathBuf::from("discovered.json")))
                    })
                    .loader(simple_config_loader())
            })
            .into_bundle(move |cfg: SimpleConfig| {
                assert_eq!(cfg.value, "test");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from(["test"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }
}

// ---------------------------------------------------------------------------------------------- //
// Query and Override Args Tests

mod query_and_override_args {
    use super::*;

    #[test]
    fn desc_exits_early_without_config() {
        // --desc should work without a config file
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new().desc("desc", None))
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file", Required::No)
                    .loader(|_: &Path| -> Result<Node, std::io::Error> {
                        panic!("loader should not be called")
                    })
            })
            .into_bundle(|_cfg: SimpleConfig| -> Result<(), std::io::Error> {
                panic!("handler should not be called");
            });

        let result = bundle.run_from(["test", "--desc"]);
        assert!(result.is_ok());
    }

    #[test]
    fn desc_with_key_validates_path() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new().desc("desc", None))
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file", Required::No)
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});

        // Invalid key should error
        let result = bundle.run_from(["test", "--desc", "invalid.key"]);
        assert!(result.is_err());
    }

    #[test]
    fn get_outputs_tree_format() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new().get("get", None))
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(|_cfg: NestedConfig| -> Result<(), std::io::Error> {
                panic!("handler should not be called with --get");
            });

        // Should succeed and exit early
        let result = bundle.run_from(["test", "config.json", "--get"]);
        assert!(result.is_ok());
    }

    #[test]
    fn get_with_key() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new().get("get", None))
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(|_cfg: NestedConfig| -> Result<(), std::io::Error> {
                panic!("handler should not be called");
            });

        let result = bundle.run_from(["test", "config.json", "--get", "database.host"]);
        assert!(result.is_ok());
    }

    #[test]
    fn get_flat_outputs_flat_format() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new().get_flat("get-flat", None))
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(|_cfg: NestedConfig| -> Result<(), std::io::Error> {
                panic!("handler should not be called");
            });

        let result = bundle.run_from(["test", "config.json", "--get-flat"]);
        assert!(result.is_ok());
    }

    #[test]
    fn get_as_json() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new().get_as("get-as", None))
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(|_cfg: NestedConfig| -> Result<(), std::io::Error> {
                panic!("handler should not be called");
            });

        let result = bundle.run_from(["test", "config.json", "--get-as", "json"]);
        assert!(result.is_ok());
    }

    #[test]
    fn get_as_rhai() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new().get_as("get-as", None))
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(|_cfg: NestedConfig| -> Result<(), std::io::Error> {
                panic!("handler should not be called");
            });

        let result = bundle.run_from(["test", "config.json", "--get-as", "rhai"]);
        assert!(result.is_ok());
    }

    #[test]
    fn get_as_unknown_format_errors() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new().get_as("get-as", None))
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(|_cfg: NestedConfig| {});

        let result = bundle.run_from(["test", "config.json", "--get-as", "ini"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, SetupError::UnknownGetAsFormat { .. }),
            "Expected SetupError::UnknownGetAsFormat, got {:?}",
            err
        );
    }

    #[test]
    fn set_modifies_config() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new().set("set", None))
            .query_args(QueryArgs::new())
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(move |cfg: NestedConfig| {
                assert_eq!(cfg.database.host, "remotehost");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from([
            "test",
            "config.json",
            "--set",
            "database.host",
            "remotehost",
        ]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn set_multiple_values() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new().set("set", None))
            .query_args(QueryArgs::new())
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(move |cfg: NestedConfig| {
                assert_eq!(cfg.database.host, "remotehost");
                assert_eq!(cfg.name, "changed");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from([
            "test",
            "config.json",
            "--set",
            "database.host",
            "remotehost",
            "--set",
            "name",
            "changed",
        ]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn remove_removes_key() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new().remove("remove", None))
            .query_args(QueryArgs::new())
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(move |cfg: NestedConfig| {
                // Port should fall back to default after removal
                assert_eq!(cfg.database.port, 5432);
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from(["test", "config.json", "--remove", "database.port"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn remove_nonexistent_path_errors() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new().remove("remove", None))
            .query_args(QueryArgs::new())
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(|_cfg: NestedConfig| {});

        let result = bundle.run_from(["test", "config.json", "--remove", "database.nope"]);
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("does not exist"), "expected 'does not exist' in: {msg}");
    }

    #[test]
    fn remove_nonexistent_path_shows_hint() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new().remove("remove", None))
            .query_args(QueryArgs::new())
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(|_cfg: NestedConfig| {});

        let result = bundle.run_from(["test", "config.json", "--remove", "database.nope"]);
        let err = result.unwrap_err();
        let msg = err.to_string();
        // The hint from UnknownPathError should mention available fields.
        assert!(msg.contains("Unknown key"), "expected 'Unknown key' hint in: {msg}");
        assert!(msg.contains("host"), "expected 'host' in available fields hint: {msg}");
    }

    #[test]
    fn standard_args_have_all_options() {
        let query_spec = QueryArgs::standard();
        let query_names: Vec<_> = query_spec.all_names().collect();
        assert!(query_names.contains(&"desc"));
        assert!(query_names.contains(&"get"));
        assert!(query_names.contains(&"get-flat"));
        assert!(query_names.contains(&"get-as"));

        let override_spec = OverrideArgs::standard();
        let override_names: Vec<_> = override_spec.all_names().collect();
        assert!(override_names.contains(&"set"));
        assert!(override_names.contains(&"remove"));
    }
}

// ---------------------------------------------------------------------------------------------- //
// Expose Args Tests

mod expose_args {
    use super::*;

    #[test]
    fn expose_option_overrides_value() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.option("database.port");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(move |cfg: NestedConfig| {
                assert_eq!(cfg.database.port, 9999);
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from(["test", "config.json", "--database-port", "9999"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn expose_true_flag_sets_true() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.flag("verbose", true);
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(move |cfg: NestedConfig| {
                assert!(cfg.verbose);
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from(["test", "config.json", "--verbose"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn absent_flag_leaves_config_unchanged() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.flag("verbose", true);
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(|cfg: NestedConfig| assert!(!cfg.verbose));

        let result = bundle.run_from(["test", "config.json"]);
        assert!(result.is_ok());
    }

    #[test]
    fn false_flag_disables_true_config_value() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.flag("verbose", false).long("quiet").short('Q');
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(json_loader(
                        r#"{"name":"myapp","database":{"host":"localhost","port":5432},"verbose":true}"#,
                    ))
            })
            .into_bundle(|cfg: NestedConfig| assert!(!cfg.verbose));

        let result = bundle.run_from(["test", "config.json", "-Q"]);
        assert!(result.is_ok());
    }

    #[test]
    fn flag_accepts_enum_name_without_consuming_a_value() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.flag("order", "col_major").long("column-major");
            })
            .into_bundle(|cfg: EnumExposeConfig| {
                assert_eq!(cfg.order, ProcessOrder::ColMajor);
            });

        let result = bundle.run_from(["test", "--column-major"]);
        assert!(result.is_ok());
    }

    #[test]
    fn flag_accepts_numeric_and_structured_values() {
        #[derive(Debug, Config)]
        struct FlagConfig {
            /// Flag numeric value.
            port: u16,
            /// Flag structured value.
            items: Vec<String>,
        }

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.flag("port", 8080_u16).long("default-port");
                e.flag("items", vec!["a", "b"]).long("starter-items");
            })
            .into_bundle(|cfg: FlagConfig| {
                assert_eq!(cfg.port, 8080);
                assert_eq!(cfg.items, vec!["a", "b"]);
            });

        let result = bundle.run_from(["test", "--default-port", "--starter-items"]);
        assert!(result.is_ok());
    }

    #[test]
    fn flag_help_has_no_value_placeholder() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.flag("verbose", false).long("quiet").short('Q').help("Disables verbose output.");
            })
            .into_bundle(|_cfg: NestedConfig| {});

        let mut command = bundle.command().clone();
        let help = command.render_long_help().to_string();

        assert!(help.contains("-Q, --quiet"), "was:\n{help}");
        assert!(!help.contains("--quiet <"), "was:\n{help}");
    }

    #[test]
    fn expose_with_custom_long() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.option("database.port").long("port");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(move |cfg: NestedConfig| {
                assert_eq!(cfg.database.port, 8080);
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from(["test", "config.json", "--port", "8080"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn expose_with_short() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.flag("verbose", true).short('v');
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(nested_config_loader())
            })
            .into_bundle(move |cfg: NestedConfig| {
                assert!(cfg.verbose);
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from(["test", "config.json", "-v"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn expose_arg_name_conversion() {
        // Test the kebab-case conversion via the generated command
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.option("foo_bar");
                e.option("nested.value");
                e.option("some_nested.path_here");
                e.option("CamelCase"); // CamelCase
                e.option("nested.mixedCase"); // Mixed dot and camelCase
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});

        let args: Vec<_> = bundle.command().get_arguments().collect();
        let long_names: Vec<_> = args.iter().filter_map(|a| a.get_long()).collect();

        // heck converts various cases to kebab-case
        assert!(long_names.contains(&"foo-bar")); // foo_bar -> foo-bar
        assert!(long_names.contains(&"nested-value")); // nested.value -> nested-value
        assert!(long_names.contains(&"some-nested-path-here")); // some_nested.path_here -> some-nested-path-here
        assert!(long_names.contains(&"camel-case")); // CamelCase -> camel-case
        assert!(long_names.contains(&"nested-mixed-case")); // nested.mixedCase -> nested-mixed-case
    }

    #[test]
    fn expose_list_appends_to_existing() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.list("items");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(json_loader(r#"{"items": ["a"]}"#))
            })
            .into_bundle(move |cfg: ListTestConfig| {
                assert_eq!(cfg.items, vec!["a", "b", "c"]);
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from(["test", "config.json", "--items", "b", "--items", "c"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn expose_list_creates_when_missing() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.list("items");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(json_loader(r#"{}"#))
            })
            .into_bundle(move |cfg: ListTestConfig| {
                assert_eq!(cfg.items, vec!["a"]);
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from(["test", "config.json", "--items", "a"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn expose_list_errors_on_scalar() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.list("items");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(json_loader(r#"{"items": "x"}"#))
            })
            .into_bundle(|_cfg: ListTestConfig| {});

        let result = bundle.run_from(["test", "config.json", "--items", "y"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("expected array"), "expected 'expected array' in: {msg}");
    }

    #[test]
    fn expose_list_preserves_argv_order() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.list("items");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(json_loader(r#"{}"#))
            })
            .into_bundle(move |cfg: ListTestConfig| {
                assert_eq!(cfg.items, vec!["c", "a", "b"]);
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from([
            "test",
            "config.json",
            "--items",
            "c",
            "--items",
            "a",
            "--items",
            "b",
        ]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn expose_unit_enum_help_lists_possible_values() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.option("order");
            })
            .into_bundle(|_cfg: EnumExposeConfig| {});

        let mut command = bundle.command().clone();
        let help = command.render_long_help().to_string();

        assert!(help.contains("Possible values:"));
        assert!(help.contains("row-major"));
        assert!(help.contains("col-major"));
        assert!(help.contains("Traverses rows first."));
        assert!(help.contains("Traverses columns first."));
    }

    #[test]
    fn expose_unit_enum_rejects_invalid_cli_value_with_variants() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.option("order");
            })
            .into_bundle(|_cfg: EnumExposeConfig| {});

        let err = bundle
            .command()
            .clone()
            .try_get_matches_from(["test", "--order", "diagonal"])
            .unwrap_err();
        let message = err.to_string();

        assert!(message.contains("invalid value"));
        assert!(message.contains("row-major"));
        assert!(message.contains("col-major"));
    }

    #[test]
    fn expose_unit_enum_accepts_case_insensitive_cli_value() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.option("order");
            })
            .into_bundle(move |cfg: EnumExposeConfig| {
                assert_eq!(cfg.order, ProcessOrder::ColMajor);
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from(["test", "--order", "COL-MAJOR"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn expose_unit_enum_accepts_snake_case_alias() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.option("order");
            })
            .into_bundle(move |cfg: EnumExposeConfig| {
                assert_eq!(cfg.order, ProcessOrder::ColMajor);
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from(["test", "--order", "col_major"]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }
}

// ---------------------------------------------------------------------------------------------- //
// Collision Detection Tests
//
// These tests verify that argument collisions panic with a clear error message.

mod collision_detection {
    use super::*;

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn duplicate_id_panics() {
        Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new().get("get", None))
            .expose(|e: &mut ExposeMap| {
                e.option("get"); // Collides with --get query arg
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn expose_collides_with_config_flag_panics() {
        Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .option("config", None, "Path to config file")
                    .loader(simple_config_loader())
            })
            .expose(|e: &mut ExposeMap| {
                e.option("config"); // Collides with --config
            })
            .into_bundle(|_cfg: SimpleConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn duplicate_extra_arg_ids_panics() {
        use clap::Arg;

        Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .arg(Arg::new("output").long("output"))
            .arg(Arg::new("output").long("output2")) // Same ID, different long
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn extra_arg_long_collides_with_config_option_panics() {
        use clap::Arg;

        Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .option("config", None, "Path to config file")
                    .loader(simple_config_loader())
            })
            .arg(Arg::new("cfg").long("config")) // ID is "cfg", but long is "config"
            .into_bundle(|_cfg: SimpleConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn short_flag_collision_panics() {
        Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new().desc("desc", Some('d')))
            .expose(|e: &mut ExposeMap| {
                e.flag("verbose", true).short('d'); // Same short as --desc
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn extra_arg_id_collides_with_query_arg_id_panics() {
        use clap::Arg;

        Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new().get("get", None))
            .arg(Arg::new("get").long("fetch")) // ID is "get", long is "fetch"
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn extra_arg_id_collides_with_config_option_id_panics() {
        use clap::Arg;

        Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .option("config", None, "Path to config file")
                    .loader(simple_config_loader())
            })
            .arg(Arg::new("config").long("settings")) // ID is "config", long is "settings"
            .into_bundle(|_cfg: SimpleConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn extra_arg_id_collides_with_expose_arg_id_panics() {
        use clap::Arg;

        Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.flag("verbose", true); // Creates arg with ID "verbose" and long "verbose"
            })
            .arg(Arg::new("verbose").long("debug")) // ID is "verbose", long is "debug"
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn config_positional_id_collides_with_config_option_id_panics() {
        Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .positional("config", "Path to config file", Required::Yes)
                    .option("config", None, "Path to config file")
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});
    }
}

// ---------------------------------------------------------------------------------------------- //
// Setup Behavior Tests

mod setup_behavior {
    use super::*;

    #[test]
    fn config_source_preserves_and_transforms() {
        // config_source() receives the existing ConfigSource and transforms it.

        let bundle = Setup::standard("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            // standard() has positional("CONFIG") - calling config_source() replaces it.
            .config_source(|_| {
                ConfigSource::new()
                    .positional("FILE", "Path to config file", Required::No)
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});

        // Verify the command uses "FILE" (the replacement)
        let args: Vec<_> = bundle.command().get_positionals().collect();
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].get_id().as_str(), "FILE");
    }

    #[test]
    fn config_source_can_add_option_to_existing() {
        // Show that config_source() can add to existing spec, not just replace.

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file", Required::No)
                    .option("config", Some('c'), "Path to config file")
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});

        // Should have both positional and --config flag
        let positionals: Vec<_> = bundle.command().get_positionals().collect();
        assert_eq!(positionals.len(), 1);
        assert_eq!(positionals[0].get_id().as_str(), "CONFIG");

        let args: Vec<_> = bundle.command().get_arguments().collect();
        let long_names: Vec<_> = args.iter().filter_map(|a| a.get_long()).collect();
        assert!(long_names.contains(&"config"));
    }

    #[test]
    fn expose_setup_accumulates() {
        // Multiple expose() calls accumulate fields

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.option("first");
            })
            .expose(|e: &mut ExposeMap| {
                e.option("second");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});

        // Both "first" and "second" should be exposed
        let args: Vec<_> = bundle.command().get_arguments().collect();
        let long_names: Vec<_> = args.iter().filter_map(|a| a.get_long()).collect();
        assert!(long_names.contains(&"first"));
        assert!(long_names.contains(&"second"));
    }

    #[test]
    fn new_has_no_positional_config() {
        // Setup::new() has no config source, so no positional.
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new())
            .query_args(QueryArgs::new())
            .into_bundle(|_cfg: SimpleConfig| {});

        let args: Vec<_> = bundle.command().get_positionals().collect();
        assert_eq!(args.len(), 0);
    }

    #[test]
    fn standard_setup_has_positional_config() {
        // Setup::standard() includes a positional CONFIG argument.
        let bundle = Setup::standard("test").into_bundle(|_cfg: SimpleConfig| {});

        let args: Vec<_> = bundle.command().get_positionals().collect();
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].get_id().as_str(), "CONFIG");
    }

    #[test]
    fn standard_setup_has_standard_args() {
        let bundle = Setup::standard("test").into_bundle(|_cfg: SimpleConfig| {});

        let args: Vec<_> = bundle.command().get_arguments().collect();
        let long_names: Vec<_> = args.iter().filter_map(|a| a.get_long()).collect();

        assert!(long_names.contains(&"desc"));
        assert!(long_names.contains(&"set"));
        assert!(long_names.contains(&"remove"));
        assert!(long_names.contains(&"get"));
        assert!(long_names.contains(&"get-flat"));
        assert!(long_names.contains(&"get-as"));
    }
}

// ---------------------------------------------------------------------------------------------- //
// Argv Ordering Tests
//
// Tests that verify CLI overrides are applied in command-line order.

mod argv_ordering {
    use super::*;

    #[derive(Debug, Config)]
    struct OrderTestConfig {
        /// A value that can be set multiple times.
        value: String,
        /// A secondary value.
        other: String,
    }

    fn order_config_loader() -> impl Fn(&Path) -> Result<Node, std::convert::Infallible> {
        move |_path| Ok(Node::parse_str(r#"{ "value": "", "other": "" }"#, Format::Json).unwrap())
    }

    #[test]
    fn set_then_set_last_wins() {
        // Multiple --set for the same key: last one wins
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new().set("set", None))
            .query_args(QueryArgs::new())
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(order_config_loader())
            })
            .into_bundle(move |cfg: OrderTestConfig| {
                assert_eq!(cfg.value, "b", "last --set should win");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from([
            "test",
            "config.json",
            "--set",
            "value",
            "a",
            "--set",
            "value",
            "b",
        ]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn flag_then_set_last_wins() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new().set("set", None))
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.flag("value", "from-flag").long("flag-value");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(order_config_loader())
            })
            .into_bundle(|cfg: OrderTestConfig| assert_eq!(cfg.value, "from-set"));

        let result = bundle.run_from([
            "test",
            "config.json",
            "--flag-value",
            "--set",
            "value",
            "from-set",
        ]);
        assert!(result.is_ok());
    }

    #[test]
    fn set_then_flag_last_wins() {
        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new().set("set", None))
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.flag("value", "from-flag").long("flag-value");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(order_config_loader())
            })
            .into_bundle(|cfg: OrderTestConfig| assert_eq!(cfg.value, "from-flag"));

        let result = bundle.run_from([
            "test",
            "config.json",
            "--set",
            "value",
            "from-set",
            "--flag-value",
        ]);
        assert!(result.is_ok());
    }

    #[test]
    fn expose_then_set_last_wins() {
        // Exposed option followed by --set for same key: --set should win
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new().set("set", None))
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.option("value");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(order_config_loader())
            })
            .into_bundle(move |cfg: OrderTestConfig| {
                assert_eq!(cfg.value, "from-set", "--set after --value should win");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        // --value from-expose comes first, then --set value from-set
        let result = bundle.run_from([
            "test",
            "config.json",
            "--value",
            "from-expose",
            "--set",
            "value",
            "from-set",
        ]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn set_then_expose_last_wins() {
        // --set followed by exposed option for same key: exposed should win
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new().set("set", None))
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.option("value");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(order_config_loader())
            })
            .into_bundle(move |cfg: OrderTestConfig| {
                assert_eq!(cfg.value, "from-expose", "--value after --set should win");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        // --set value from-set comes first, then --value from-expose
        let result = bundle.run_from([
            "test",
            "config.json",
            "--set",
            "value",
            "from-set",
            "--value",
            "from-expose",
        ]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }

    #[test]
    fn interleaved_operations() {
        // Multiple operations affecting different keys, all in argv order
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_clone = handler_called.clone();

        let bundle = Setup::new("test")
            .override_args(OverrideArgs::new().set("set", None))
            .query_args(QueryArgs::new())
            .expose(|e: &mut ExposeMap| {
                e.option("value");
                e.option("other");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file", Required::Yes)
                    .loader(order_config_loader())
            })
            .into_bundle(move |cfg: OrderTestConfig| {
                // value: starts "", --value a, --set value b -> b
                // other: starts "", --set other c, --other d -> d
                assert_eq!(cfg.value, "b");
                assert_eq!(cfg.other, "d");
                handler_called_clone.store(true, Ordering::SeqCst);
            });

        let result = bundle.run_from([
            "test",
            "config.json",
            "--value",
            "a",
            "--set",
            "other",
            "c",
            "--set",
            "value",
            "b",
            "--other",
            "d",
        ]);
        assert!(result.is_ok());
        assert!(handler_called.load(Ordering::SeqCst));
    }
}
