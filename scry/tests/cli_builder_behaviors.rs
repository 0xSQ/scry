//! Covers config-source policies, exposed arguments, and setup composition.
#![cfg(feature = "format-json")]

use std::path::{Path, PathBuf};

use scry::cli::setup::{
    ConfigSource, ExposeMap, MissingConfigPolicy, MultiSourcePolicy, Setup, SetupError,
};
use scry::node::Format;
use scry::{Config, Describe, FromNode, Node, ToNode};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromNode, ToNode, Describe)]
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

#[derive(Debug, Config)]
struct PresetConfig {
    /// Runs without executing the real operation.
    #[scry(default = false)]
    dry_run: bool,
    /// Prints detailed output.
    #[scry(default = false)]
    verbose: bool,
    /// Limits the selected rows.
    row_max: Option<usize>,
    /// Limits the selected columns.
    col_max: Option<usize>,
}

// ---------------------------------------------------------------------------------------------- //
// Config Path Resolution Tests

mod config_path_resolution {
    use super::*;

    #[test]
    fn explicit_sources_select_their_paths_and_skip_discovery() {
        for arguments in [
            vec!["test", "positional.json"],
            vec!["test", "--config", "option.json"],
            vec!["test", "-c", "option.json"],
            vec!["test", "positional.json", "--config", "option.json"],
        ] {
            for policy in [
                MultiSourcePolicy::PreferPositional,
                MultiSourcePolicy::PreferOption,
            ] {
                let source = ConfigSource::new()
                    .positional("CONFIG", "Path to config file.")
                    .option("config", Some('c'), "Path to config file.")
                    .multi_source_policy(policy)
                    .discover(|_| -> Result<Option<PathBuf>, std::convert::Infallible> {
                        panic!("explicit paths must skip discovery")
                    })
                    .loader(|path: &Path| -> Result<Node, std::convert::Infallible> {
                        let mut node = Node::empty_map();
                        node.set_value("value", path.to_str().unwrap()).unwrap();
                        Ok(node)
                    });
                let value = Setup::new("test")
                    .config_source(|_| source)
                    .into_bundle(|config: SimpleConfig| config.value)
                    .run_from(&arguments)
                    .unwrap()
                    .unwrap();
                let expected = if arguments.len() == 2
                    || (arguments.len() == 4 && policy == MultiSourcePolicy::PreferPositional)
                {
                    "positional.json"
                } else {
                    "option.json"
                };
                assert_eq!(value, expected, "{policy:?}, {arguments:?}");
            }
        }
    }

    #[test]
    fn multiple_explicit_paths_are_rejected_by_default() {
        let error = Setup::new("test")
            .config_source(|source| {
                source
                    .positional("CONFIG", "Path to config file.")
                    .option("config", None, "Path to config file.")
                    .discover(|_| -> Result<Option<PathBuf>, std::convert::Infallible> {
                        panic!("conflicting paths must skip discovery")
                    })
                    .loader(|_: &Path| -> Result<Node, std::convert::Infallible> {
                        panic!("conflicting paths must skip loading")
                    })
            })
            .into_bundle(|_: SimpleConfig| panic!("conflicting paths must skip execution"))
            .run_from(["test", "positional.json", "--config", "option.json"])
            .unwrap_err();
        assert!(matches!(error, SetupError::MultipleConfigPaths));
    }

    #[test]
    fn discovery_precedes_the_missing_input_policy() {
        for policy in [MissingConfigPolicy::Error, MissingConfigPolicy::EmptyNode] {
            let value = Setup::new("test")
                .config_source(|source| {
                    source
                        .missing_policy(policy)
                        .discover(|command| -> Result<Option<PathBuf>, std::convert::Infallible> {
                            assert_eq!(command, "test");
                            Ok(Some(PathBuf::from("discovered.json")))
                        })
                        .loader(|path: &Path| {
                            assert_eq!(path, Path::new("discovered.json"));
                            simple_config_loader()(path)
                        })
                })
                .into_bundle(|config: SimpleConfig| config.value)
                .run_from(["test"])
                .unwrap()
                .unwrap();
            assert_eq!(value, "test");
        }
    }

    #[test]
    fn discovery_errors_preserve_their_cause_even_with_empty_input_enabled() {
        let error = Setup::new("test")
            .config_source(|source| {
                source
                    .or_empty()
                    .discover(|_| Err::<Option<PathBuf>, _>(anyhow::anyhow!("discovery failed")))
            })
            .into_bundle(|_: SimpleConfig| panic!("failed discovery must skip execution"))
            .run_from(["test"])
            .unwrap_err();
        let SetupError::ConfigNotFound { cmd_name, source } = error else {
            panic!("expected a discovery error");
        };
        assert_eq!(cmd_name, "test");
        assert_eq!(source.to_string(), "discovery failed");
    }

    #[test]
    fn absent_input_obeys_the_missing_policy_with_or_without_discovery() {
        for discover in [false, true] {
            for policy in [MissingConfigPolicy::Error, MissingConfigPolicy::EmptyNode] {
                let result = Setup::new("test")
                    .config_source(|mut source| {
                        if discover {
                            source = source.discover(|_| Ok::<_, std::convert::Infallible>(None));
                        }
                        source.missing_policy(policy).loader(
                            |_: &Path| -> Result<Node, std::convert::Infallible> {
                                panic!("absent input must skip loading")
                            },
                        )
                    })
                    .into_bundle(|config: SimpleConfig| config.value)
                    .run_from(["test", "--scry", "--set", "value", "from-cli"]);
                match policy {
                    MissingConfigPolicy::Error => {
                        assert!(matches!(result, Err(SetupError::NoConfigFile { .. })));
                    }
                    MissingConfigPolicy::EmptyNode => {
                        assert_eq!(result.unwrap().unwrap(), "from-cli");
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Support Inspection and Edit Tests

mod support_actions {
    use super::*;

    #[test]
    fn desc_with_key_validates_path() {
        let bundle = Setup::new("test")
            .config_source(|_| {
                ConfigSource::new()
                    .positional("CONFIG", "Path to config file.")
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});

        // Invalid key should error
        let result = bundle.run_from(["test", "--scry", "desc", "invalid.key"]);
        assert!(result.is_err());
    }

    #[test]
    fn remove_removes_key() {
        let bundle = Setup::new("test")
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file.").loader(json_loader(
                    r#"{ "name": "app", "database": { "host": "localhost", "port": 9000 } }"#,
                ))
            })
            .into_bundle(move |cfg: NestedConfig| {
                // Removing a loaded value restores its typed default.
                assert_eq!(cfg.database.port, 5432);
            });

        let result =
            bundle.run_from(["test", "config.json", "--scry", "--remove", "database.port"]);
        assert!(matches!(result, Ok(Some(()))));
    }

    #[test]
    fn removing_an_absent_path_reports_the_missing_key_and_available_fields() {
        let bundle = Setup::new("test")
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file.").loader(nested_config_loader())
            })
            .into_bundle(|_cfg: NestedConfig| {});

        let result =
            bundle.run_from(["test", "config.json", "--scry", "--remove", "database.nope"]);
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("does not exist"), "{msg}");
        assert!(msg.contains("Unknown key"), "expected 'Unknown key' hint in: {msg}");
        assert!(msg.contains("host"), "expected 'host' in available fields hint: {msg}");
    }
}

// ---------------------------------------------------------------------------------------------- //
// Expose Args Tests

mod expose_args {
    use super::*;

    #[test]
    fn expose_option_overrides_value() {
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.option("database.port");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file.").loader(nested_config_loader())
            })
            .into_bundle(move |cfg: NestedConfig| {
                assert_eq!(cfg.database.port, 9999);
                assert_eq!(cfg.database.host, "localhost");
            });

        let result = bundle.run_from(["test", "config.json", "--database-port", "9999"]);
        assert!(matches!(result, Ok(Some(()))));
    }

    #[test]
    fn expose_true_flag_sets_true() {
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.flag("verbose", true);
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file.").loader(nested_config_loader())
            })
            .into_bundle(move |cfg: NestedConfig| {
                assert!(cfg.verbose);
            });

        let result = bundle.run_from(["test", "config.json", "--verbose"]);
        assert!(matches!(result, Ok(Some(()))));
    }

    #[test]
    fn absent_flag_leaves_config_unchanged() {
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.flag("verbose", true);
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file.").loader(nested_config_loader())
            })
            .into_bundle(|cfg: NestedConfig| assert!(!cfg.verbose));

        let result = bundle.run_from(["test", "config.json"]);
        assert!(result.is_ok());
    }

    #[test]
    fn false_flag_disables_true_config_value() {
        let bundle = Setup::new("test")

            .expose(|e: &mut ExposeMap| {
                e.flag("verbose", false).long("quiet").short('Q');
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file.")
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
    fn preset_applies_all_fixed_assignments() {
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.preset("probe")
                    .set("dry_run", true)
                    .set("verbose", true)
                    .set("row_max", 1_usize)
                    .set("col_max", 1_usize);
            })
            .into_bundle(|cfg: PresetConfig| {
                assert!(cfg.dry_run);
                assert!(cfg.verbose);
                assert_eq!(cfg.row_max, Some(1));
                assert_eq!(cfg.col_max, Some(1));
            });

        assert!(bundle.run_from(["test", "--probe"]).is_ok());
    }

    #[test]
    fn absent_preset_leaves_config_unchanged() {
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.preset("probe")
                    .set("dry_run", true)
                    .set("verbose", true)
                    .set("row_max", 1_usize)
                    .set("col_max", 1_usize);
            })
            .into_bundle(|cfg: PresetConfig| {
                assert!(!cfg.dry_run);
                assert!(!cfg.verbose);
                assert_eq!(cfg.row_max, None);
                assert_eq!(cfg.col_max, None);
            });

        assert!(bundle.run_from(["test"]).is_ok());
    }

    #[test]
    fn preset_uses_explicit_name_and_help() {
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.preset("quick_probe")
                    .set("dry_run", true)
                    .set("verbose", true)
                    .help("Inspects one setup without running it.");
            })
            .into_bundle(|_cfg: PresetConfig| {});

        let mut command = bundle.command().clone();
        let help = command.render_long_help().to_string();

        assert!(help.contains("--quick-probe"), "was:\n{help}");
        assert!(help.contains("Inspects one setup without running it."), "was:\n{help}");
        assert!(!help.contains("--quick-probe <"), "was:\n{help}");
    }

    #[test]
    #[should_panic(expected = "must contain at least one fixed assignment")]
    fn empty_preset_panics_when_building_command() {
        Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.preset("empty");
            })
            .into_bundle(|_cfg: PresetConfig| {});
    }

    #[test]
    #[should_panic(expected = "already assigns config path 'verbose'")]
    fn duplicate_preset_assignment_panics() {
        let mut map = ExposeMap::new();
        map.preset("probe").set("verbose", true).set("verbose", false);
    }

    #[test]
    #[should_panic(expected = "cannot add fixed assignment")]
    fn set_on_option_panics() {
        let mut map = ExposeMap::new();
        map.option("verbose").set("dry_run", true);
    }

    #[test]
    fn expose_with_custom_long() {
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.option("database.port").long("port");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file.").loader(nested_config_loader())
            })
            .into_bundle(move |cfg: NestedConfig| {
                assert_eq!(cfg.database.port, 8080);
            });

        let result = bundle.run_from(["test", "config.json", "--port", "8080"]);
        assert!(matches!(result, Ok(Some(()))));
    }

    #[test]
    fn expose_with_short() {
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.flag("verbose", true).short('v');
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file.").loader(nested_config_loader())
            })
            .into_bundle(move |cfg: NestedConfig| {
                assert!(cfg.verbose);
            });

        let result = bundle.run_from(["test", "config.json", "-v"]);
        assert!(matches!(result, Ok(Some(()))));
    }

    #[test]
    fn expose_arg_name_conversion() {
        // Test the kebab-case conversion via the generated command
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.option("foo_bar");
                e.option("nested.value");
                e.option("some_nested.path_here");
                e.option("CamelCase"); // CamelCase
                e.option("nested.mixedCase"); // Mixed dot and camelCase
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file.").loader(simple_config_loader())
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
    fn expose_unit_enum_help_lists_possible_values() {
        let bundle = Setup::new("test")
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
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.option("order");
            })
            .into_bundle(move |cfg: EnumExposeConfig| {
                assert_eq!(cfg.order, ProcessOrder::ColMajor);
            });

        let result = bundle.run_from(["test", "--order", "COL-MAJOR"]);
        assert!(matches!(result, Ok(Some(()))));
    }

    #[test]
    fn expose_unit_enum_accepts_snake_case_alias() {
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.option("order");
            })
            .into_bundle(move |cfg: EnumExposeConfig| {
                assert_eq!(cfg.order, ProcessOrder::ColMajor);
            });

        let result = bundle.run_from(["test", "--order", "col_major"]);
        assert!(matches!(result, Ok(Some(()))));
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
    fn exposed_option_collides_with_support_long_name() {
        Setup::new("test")
            .expose(|e| {
                e.option("value").long("scry");
            })
            .into_bundle(|_cfg: SimpleConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn preset_collides_with_exposed_flag_panics() {
        Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.flag("verbose", true);
                e.preset("verbose").set("value", "preset");
            })
            .into_bundle(|_cfg: NestedConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn expose_collides_with_config_flag_panics() {
        Setup::new("test")
            .config_source(|_| {
                ConfigSource::new()
                    .option("config", None, "Path to config file.")
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
            .arg(Arg::new("output").long("output"))
            .arg(Arg::new("output").long("output2")) // Same ID, different long
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file.").loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn extra_arg_long_collides_with_config_option_panics() {
        use clap::Arg;

        Setup::new("test")
            .config_source(|_| {
                ConfigSource::new()
                    .option("config", None, "Path to config file.")
                    .loader(simple_config_loader())
            })
            .arg(Arg::new("cfg").long("config")) // ID is "cfg", but long is "config"
            .into_bundle(|_cfg: SimpleConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn short_flag_collision_panics() {
        Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.flag("verbose", true).short('S'); // Shares the support switch's short alias.
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file.").loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn extra_arg_id_collides_with_support_id_panics() {
        use clap::Arg;

        Setup::new("test")
            .arg(Arg::new("scry_support").long("fetch")) // Shares the support argument ID.
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file.").loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn extra_arg_id_collides_with_config_option_id_panics() {
        use clap::Arg;

        Setup::new("test")
            .config_source(|_| {
                ConfigSource::new()
                    .option("config", None, "Path to config file.")
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
            .expose(|e: &mut ExposeMap| {
                e.flag("verbose", true); // Creates arg with ID "verbose" and long "verbose"
            })
            .arg(Arg::new("verbose").long("debug")) // ID is "verbose", long is "debug"
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file.").loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn config_positional_id_collides_with_config_option_id_panics() {
        Setup::new("test")
            .config_source(|_| {
                ConfigSource::new()
                    .positional("config", "Path to config file.")
                    .option("config", None, "Path to config file.")
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
    fn config_source_can_replace_standard_source() {
        let bundle = Setup::standard("test")
            // Replacing the source removes the standard named config option.
            .config_source(|_| {
                ConfigSource::new()
                    .positional("FILE", "Path to config file.")
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});

        let args: Vec<_> = bundle.command().get_positionals().collect();
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].get_id().as_str(), "FILE");
        assert!(!bundle.command().get_arguments().any(|arg| arg.get_long() == Some("config")));
    }

    #[test]
    fn config_source_can_add_option_to_existing() {
        let bundle = Setup::new("test")
            .config_source(|source| source.positional("CONFIG", "Path to config file."))
            .config_source(|source| {
                source
                    .option("config", Some('c'), "Path to config file.")
                    .loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});

        let positionals: Vec<_> = bundle.command().get_positionals().collect();
        assert_eq!(positionals.len(), 1);
        assert_eq!(positionals[0].get_id().as_str(), "CONFIG");

        let args: Vec<_> = bundle.command().get_arguments().collect();
        let long_names: Vec<_> = args.iter().filter_map(|a| a.get_long()).collect();
        assert!(long_names.contains(&"config"));
    }

    #[test]
    fn expose_setup_accumulates() {
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.option("first");
            })
            .expose(|e: &mut ExposeMap| {
                e.option("second");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file.").loader(simple_config_loader())
            })
            .into_bundle(|_cfg: SimpleConfig| {});

        let args: Vec<_> = bundle.command().get_arguments().collect();
        let long_names: Vec<_> = args.iter().filter_map(|a| a.get_long()).collect();
        assert!(long_names.contains(&"first"));
        assert!(long_names.contains(&"second"));
    }

    #[test]
    fn new_has_no_positional_config() {
        let bundle = Setup::new("test").into_bundle(|_cfg: SimpleConfig| {});

        let args: Vec<_> = bundle.command().get_positionals().collect();
        assert_eq!(args.len(), 0);
    }

    #[test]
    fn standard_setup_has_named_config_source() {
        let bundle = Setup::standard("test").into_bundle(|_cfg: SimpleConfig| {});

        assert_eq!(bundle.command().get_positionals().count(), 0);
        let argument =
            bundle.command().get_arguments().find(|arg| arg.get_long() == Some("config")).unwrap();
        assert_eq!(argument.get_short(), Some('C'));
        assert!(!argument.is_required_set());

        let matches = bundle
            .command()
            .clone()
            .try_get_matches_from(["test", "--config", "config.rhai::settings"])
            .unwrap();
        assert_eq!(
            matches.get_one::<PathBuf>("config"),
            Some(&PathBuf::from("config.rhai::settings"))
        );
    }
}

// ---------------------------------------------------------------------------------------------- //
// Argv Ordering Tests
//
// Application edits and edits after the support switch each retain their command-line order.

mod argv_ordering {
    use super::*;

    #[derive(Debug, Config)]
    struct OrderTestConfig {
        /// A value that can be set multiple times.
        value: String,
        /// A secondary value.
        other: String,
    }

    #[test]
    fn application_options_and_flags_follow_argv_order() {
        assert_eq!(run(&["--flag-value", "--value", "from-option"]).unwrap().value, "from-option");
        assert_eq!(run(&["--value", "from-option", "--flag-value"]).unwrap().value, "from-flag");
    }

    #[test]
    fn application_presets_and_options_follow_argv_order() {
        let config = run(&["--bundle", "--value", "from-option"]).unwrap();
        assert_eq!(config.value, "from-option");
        assert_eq!(config.other, "preset-other");

        let config = run(&["--value", "from-option", "--bundle"]).unwrap();
        assert_eq!(config.value, "from-preset");
        assert_eq!(config.other, "preset-other");
    }

    #[test]
    fn suffix_edits_follow_all_application_edits() {
        let config = run(&[
            "--value",
            "option",
            "--bundle",
            "--flag-value",
            "--other",
            "application",
            "--scry",
            "--set",
            "other",
            "suffix-other",
            "--set",
            "value",
            "suffix-value",
        ])
        .unwrap();
        assert_eq!(config.value, "suffix-value");
        assert_eq!(config.other, "suffix-other");
    }

    #[test]
    fn suffix_edit_order_is_preserved() {
        let config = run(&[
            "--scry", "--set", "value", "first", "--set", "value", "second", "--remove", "value",
            "--set", "value", "final", "--set", "other", "edited",
        ])
        .unwrap();
        assert_eq!(config.value, "final");
        assert_eq!(config.other, "edited");
    }

    #[test]
    fn application_options_after_the_support_are_rejected() {
        let error = run(&["--scry", "--set", "value", "edited", "--value", "late"]).unwrap_err();
        assert!(matches!(error, SetupError::Support(_)));
    }

    fn run(args: &[&str]) -> Result<OrderTestConfig, SetupError> {
        let bundle = Setup::new("test")
            .expose(|e| {
                e.option("value");
                e.option("other");
                e.flag("value", "from-flag").long("flag-value");
                e.preset("bundle").set("value", "from-preset").set("other", "preset-other");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Loads the input.").loader(|_: &Path| {
                    Ok::<_, std::convert::Infallible>(
                        Node::parse_str(r#"{ "value": "", "other": "" }"#, Format::Json).unwrap(),
                    )
                })
            })
            .into_bundle(|config: OrderTestConfig| config);
        let mut argv = vec!["test", "config.json"];
        argv.extend_from_slice(args);
        Ok(bundle.run_from(argv)?.unwrap())
    }
}

// ---------------------------------------------------------------------------------------------- //
// Variant Expose Tests

mod variant_expose {
    use super::*;

    /// A data-carrying enum in Scry's standard single-key-map form.
    #[derive(Debug, PartialEq, FromNode, ToNode, Describe)]
    enum ImageSource {
        /// A grid directory.
        Grid(String),
        /// A set of image files.
        Files(String),
    }

    #[derive(Debug, Config)]
    struct SourceConfig {
        /// Where the images come from.
        source: ImageSource,
    }

    /// Runs a bundle exposing `--grid` / `--files` variant options over a prior config state,
    /// asserting the source the handler receives.
    fn assert_source(prior: &'static str, args: &[&str], expected: ImageSource) {
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.option("source").variant("grid");
                e.option("source").variant("files");
            })
            .config_source(|c| {
                c.positional("CONFIG", "Path to config file.").loader(json_loader(prior))
            })
            .into_bundle(move |cfg: SourceConfig| {
                assert_eq!(cfg.source, expected);
            });

        let mut argv = vec!["test", "config.json"];
        argv.extend_from_slice(args);
        let result = bundle.run_from(argv);
        assert!(matches!(result, Ok(Some(()))), "run failed: {result:?}");
    }

    #[test]
    fn option_variant_wraps_value() {
        assert_source(r#"{}"#, &["--grid", "g/dir"], ImageSource::Grid("g/dir".into()));
    }

    #[test]
    fn variant_replaces_other_arm_wholesale() {
        // Selecting another arm replaces the whole map, preserving its single-key invariant.
        assert_source(
            r#"{"source": {"grid": "from-config"}}"#,
            &["--files", "f"],
            ImageSource::Files("f".into()),
        );
    }

    #[test]
    fn variant_replaces_leaf_shorthand() {
        assert_source(
            r#"{"source": "loose-leaf"}"#,
            &["--files", "f"],
            ImageSource::Files("f".into()),
        );
    }

    #[test]
    fn variant_replaces_same_arm_payload() {
        // Same arm held by the config with a structurally different payload: replaced, not merged.
        assert_source(
            r#"{"source": {"files": ["a", "b"]}}"#,
            &["--files", "f"],
            ImageSource::Files("f".into()),
        );
    }

    #[test]
    fn sibling_variants_last_wins() {
        assert_source(r#"{}"#, &["--grid", "g", "--files", "f"], ImageSource::Files("f".into()));
        assert_source(r#"{}"#, &["--files", "f", "--grid", "g"], ImageSource::Grid("g".into()));
    }

    #[test]
    fn variant_then_dotted_set_grafts_into_fresh_map() {
        // A dotted --set after a variant option grafts into the freshly written single-key map,
        // even when the config file held the other arm.
        assert_source(
            r#"{"source": {"grid": "from-config"}}"#,
            &["--files", "f", "--scry", "--set", "source.files", "s"],
            ImageSource::Files("s".into()),
        );
    }

    #[test]
    fn suffix_edit_changes_the_selected_variant_payload() {
        assert_source(
            r#"{}"#,
            &[
                "--files",
                "old",
                "--grid",
                "g",
                "--scry",
                "--set",
                "source.grid",
                "edited",
            ],
            ImageSource::Grid("edited".into()),
        );
    }

    #[test]
    fn custom_long_overrides_derived_name() {
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.option("source").variant("files").long("browse");
            })
            .into_bundle(|cfg: SourceConfig| {
                assert_eq!(cfg.source, ImageSource::Files("f".into()));
            });

        assert!(bundle.run_from(["test", "--browse", "f"]).is_ok());
    }

    #[test]
    fn repeated_variant_is_last_wins_setter() {
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.option("source").variant("grid").variant("files");
            })
            .into_bundle(|cfg: SourceConfig| {
                assert_eq!(cfg.source, ImageSource::Files("f".into()));
            });

        // The derived long name follows the last key too.
        assert!(bundle.run_from(["test", "--files", "f"]).is_ok());
    }

    #[test]
    fn flag_variant_wraps_fixed_value() {
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.flag("source", "preset/dir").variant("grid").long("preset-grid");
            })
            .into_bundle(|cfg: SourceConfig| {
                assert_eq!(cfg.source, ImageSource::Grid("preset/dir".into()));
            });

        assert!(bundle.run_from(["test", "--preset-grid"]).is_ok());
    }

    #[test]
    fn positional_variant_wraps_value() {
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.positional("source").variant("grid");
            })
            .into_bundle(|cfg: SourceConfig| {
                assert_eq!(cfg.source, ImageSource::Grid("g/dir".into()));
            });

        assert!(bundle.run_from(["test", "g/dir"]).is_ok());
    }

    #[test]
    fn renamed_arm_uses_serialized_key() {
        // The variant key is the serialized config key, not the Rust variant name.
        #[derive(Debug, PartialEq, FromNode, ToNode, Describe)]
        enum RenamedSource {
            /// A directory arm, renamed in config.
            #[scry(rename = "dir")]
            Directory(String),
        }

        #[derive(Debug, Config)]
        struct RenamedConfig {
            /// The source.
            source: RenamedSource,
        }

        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.option("source").variant("dir");
            })
            .into_bundle(|cfg: RenamedConfig| {
                assert_eq!(cfg.source, RenamedSource::Directory("d".into()));
            });

        assert!(bundle.run_from(["test", "--dir", "d"]).is_ok());
    }

    #[test]
    fn variant_entry_skips_unit_enum_possible_values() {
        // A variant entry's CLI value is the arm's payload, so the unit-enum possible-values
        // parser must not constrain it even when the target field describes a unit enum. Without
        // the skip, clap would reject "anything" before Scry ever saw it. Support get finishes
        // before typed conversion, so clap acceptance is all this asserts.
        let bundle = Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.option("order").variant("custom");
            })
            .into_bundle(|_cfg: EnumExposeConfig| {});

        let result = bundle.run_from(["test", "--custom", "anything", "--scry", "get", "order"]);
        assert!(result.is_ok(), "run failed: {result:?}");
    }

    #[test]
    #[should_panic(expected = "cannot take a variant wrapper")]
    fn variant_on_preset_panics() {
        let mut map = ExposeMap::new();
        map.preset("preset-grid").set("source", "preset/dir").variant("grid");
    }

    #[test]
    #[should_panic(expected = "unless it is a one-assignment field flag")]
    fn variant_flag_cannot_gain_another_assignment() {
        Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.flag("source", "preset/dir").variant("grid").set("value", "extra");
            })
            .into_bundle(|_cfg: EnumExposeConfig| {});
    }

    #[test]
    #[should_panic(expected = "CLI argument collision")]
    fn duplicate_variant_keys_panic() {
        Setup::new("test")
            .expose(|e: &mut ExposeMap| {
                e.option("source").variant("grid");
                e.flag("source", "fixed").variant("grid");
            })
            .into_bundle(|_cfg: SourceConfig| {});
    }
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
