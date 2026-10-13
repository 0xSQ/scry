//! Integration tests for the flag-only Setup toolbox.
#![cfg(feature = "format-json")]

use std::cell::Cell;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use clap::{Arg, Command};
use scry::cli::setup::{ConfigSource, Gateway, Setup, SetupError};
use scry::cli::Bundle;
use scry::node::{Format, NodeError};
use scry::{Config, Desc, Describe, FromNode, Node};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config)]
struct Input {
    /// Selects the input value.
    value: String,
    /// Collects individual values.
    #[scry(default = Vec::new())]
    items: Vec<String>,
    /// Collects grouped pairs.
    #[scry(default = Vec::new())]
    rows: Vec<Vec<String>>,
    /// Enables detailed output.
    #[scry(default = false)]
    verbose: bool,
}

#[derive(Debug)]
struct DecodeTrap;

impl FromNode for DecodeTrap {
    fn from_node(_: &Node) -> Result<Self, NodeError> {
        panic!("inspection must not decode the application input");
    }
}

impl Describe for DecodeTrap {
    fn describe() -> Desc {
        Input::describe()
    }
}

#[derive(Debug, Config)]
struct ApplicationNames {
    /// Sets the application's own get value.
    get: String,
    /// Sets the application's own set value.
    set: String,
}

// ---------------------------------------------------------------------------------------------- //

#[test]
fn ordinary_execution_still_loads_and_calls_the_payload_once() {
    let loads = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let payload_calls = calls.clone();
    let result = loaded_setup(r#"{ "value": "loaded" }"#, loads.clone())
        .into_bundle(move |input: Input| {
            payload_calls.set(payload_calls.get() + 1);
            input.value
        })
        .run_from(["test", "-C", "input.json"])
        .unwrap();

    assert_eq!(result.as_deref(), Some("loaded"));
    assert_eq!(loads.get(), 1);
    assert_eq!(calls.get(), 1);
}

#[test]
fn parent_operations_keep_their_order_and_all_precede_suffix_edits() {
    let loads = Rc::new(Cell::new(0));
    let input =
        loaded_setup(r#"{ "value": "loaded", "items": ["loaded"], "rows": [] }"#, loads.clone())
            .expose(|expose| {
                expose.option("value");
                expose.preset("preset").set("value", "preset").set("verbose", true);
                expose.option("items").append();
                expose.option("rows").array(2).append();
            })
            .into_bundle(|input: Input| input)
            .run_from([
                "test",
                "-C",
                "input.json",
                "--value",
                "parent",
                "--preset",
                "--items",
                "first",
                "--rows",
                "a",
                "b",
                "--items",
                "second",
                "--scry",
                "--set",
                "value",
                "suffix",
                "--remove",
                "items[0]",
                "--set",
                "rows[0][1]",
                "edited",
            ])
            .unwrap()
            .unwrap();

    assert_eq!(input.value, "suffix");
    assert!(input.verbose);
    assert_eq!(input.items, ["first", "second"]);
    assert_eq!(input.rows, [vec!["a", "edited"]]);
    assert_eq!(loads.get(), 1);
}

#[test]
fn suffix_edits_preserve_order_nested_paths_and_literal_values() {
    let node = loaded_setup(
        r#"{ "database": { "host": "old" }, "dotted.key": "old", "items": ["remove", "old"], "nullable": null }"#,
        Rc::new(Cell::new(0)),
    )
    .into_bundle(|node: Node| node)
    .run_from([
        "test",
        "-C",
        "input.json",
        "--scry",
        "--set",
        "database.host",
        "intermediate",
        "--remove",
        "database.host",
        "--set",
        "database.host",
        "get",
        "--set",
        r#"["dotted.key"]"#,
        "desc",
        "--remove",
        "nullable",
        "--set",
        "nullable",
        "null",
        "--remove",
        "items[0]",
        "--set",
        "items[0]",
        "two words",
    ])
    .unwrap()
    .unwrap();

    assert_eq!(node.req::<String>("database.host").unwrap(), "get");
    assert_eq!(node.req::<String>(r#"["dotted.key"]"#).unwrap(), "desc");
    assert_eq!(node.req::<String>("nullable").unwrap(), "null");
    assert_eq!(node.req::<Vec<String>>("items").unwrap(), ["two words"]);
}

#[test]
fn edit_only_execution_still_requires_valid_typed_input() {
    let error = Setup::new("test")
        .into_bundle(|_: Input| panic!("invalid input must not reach the payload"))
        .run_from(["test", "--scry", "--set", "verbose", "true"])
        .unwrap_err();

    assert!(matches!(error, SetupError::EvaluateInputConfig { .. }));
}

#[test]
fn edit_only_execution_still_requires_a_resolved_source() {
    let error = Setup::new("test")
        .config_source(|_| ConfigSource::new().option("config-file", Some('C'), "Loads input."))
        .into_bundle(|_: Input| panic!("missing source must not reach the payload"))
        .run_from(["test", "--scry", "--set", "value", "edited"])
        .unwrap_err();

    assert!(matches!(error, SetupError::NoConfigFile { .. }));
}

#[test]
fn raw_get_loads_once_and_skips_typed_decoding_and_execution() {
    let loads = Rc::new(Cell::new(0));
    let result = loaded_setup(r#"{ "items": ["old"] }"#, loads.clone())
        .expose(|expose| {
            expose.option("value");
        })
        .into_bundle(|_: DecodeTrap| panic!("get must not call the payload"))
        .run_from([
            "test",
            "-C",
            "input.json",
            "--value",
            "parent",
            "--scry",
            "--remove",
            "value",
            "--set",
            "items[0]",
            "edited",
            "get",
            "items[0]",
        ])
        .unwrap();

    assert!(result.is_none());
    assert_eq!(loads.get(), 1);
}

#[test]
fn raw_get_does_not_materialize_typed_defaults() {
    let error = loaded_setup("{}", Rc::new(Cell::new(0)))
        .into_bundle(|_: DecodeTrap| panic!("get must not call the payload"))
        .run_from(["test", "-C", "input.json", "--scry", "get", "verbose"])
        .unwrap_err();

    assert!(matches!(error, SetupError::GetNotFound { .. }));
}

#[test]
fn raw_get_distinguishes_present_null_from_an_absent_key() {
    let result = loaded_setup(r#"{ "value": null }"#, Rc::new(Cell::new(0)))
        .into_bundle(|_: DecodeTrap| panic!("get must not call the payload"))
        .run_from(["test", "-C", "input.json", "--scry", "get", "value"])
        .unwrap();
    assert!(result.is_none());

    let error = loaded_setup("{}", Rc::new(Cell::new(0)))
        .into_bundle(|_: DecodeTrap| panic!("get must not call the payload"))
        .run_from(["test", "-C", "input.json", "--scry", "get", "value"])
        .unwrap_err();
    assert!(matches!(error, SetupError::GetNotFound { .. }));
}

#[test]
fn descriptions_and_all_help_routes_skip_source_preparation() {
    let suffixes: &[&[&str]] = &[
        &[],
        &["--help"],
        &["get", "--help"],
        &["desc", "--help"],
        &["desc"],
        &["desc", "value"],
    ];
    for suffix in suffixes {
        let mut args = vec!["test", "--scry"];
        args.extend_from_slice(suffix);
        let result = unloaded_setup()
            .into_bundle(|_: DecodeTrap| panic!("help and desc must not call the payload"))
            .run_from(args)
            .unwrap();
        assert!(result.is_none(), "unexpected execution for {suffix:?}");
    }
}

#[test]
fn invalid_toolbox_requests_fail_before_source_preparation() {
    let suffixes: &[&[&str]] = &[
        &["--set", "value", "ignored", "desc"],
        &["--remove", "value", "desc"],
        &["--set"],
        &["--set", "value"],
        &["--remove"],
        &["unknown"],
        &["get", "--flat", "--as", "json"],
        &["get", "--as", "unknown"],
        &["get", "--set", "value", "late"],
        &["--scry", "get"],
        &["--config-file", "late.json", "get"],
    ];
    for suffix in suffixes {
        let mut args = vec!["test", "--scry"];
        args.extend_from_slice(suffix);
        let error = unloaded_setup()
            .into_bundle(|_: DecodeTrap| panic!("invalid toolbox input must not execute"))
            .run_from(args)
            .unwrap_err();
        assert!(matches!(error, SetupError::Gateway(_)), "{suffix:?}: {error}");
    }
}

#[test]
fn registered_raw_get_views_work_without_decoding() {
    for suffix in [
        vec!["get", "--flat"],
        vec!["get", "--as", "json"],
        vec!["get", "--as", "rhai"],
    ] {
        let loads = Rc::new(Cell::new(0));
        let mut args = vec!["test", "-C", "input.json", "--scry"];
        args.extend(suffix);
        let result = loaded_setup(r#"{ "value": "raw" }"#, loads.clone())
            .into_bundle(|_: DecodeTrap| panic!("get must not call the payload"))
            .run_from(args)
            .unwrap();
        assert!(result.is_none());
        assert_eq!(loads.get(), 1);
    }
}

#[test]
fn default_short_gateway_executes_the_same_edit_plan() {
    for entrance in ["--scry", "-S"] {
        let value = Setup::new("test")
            .into_bundle(|input: Input| input.value)
            .run_from(["test", entrance, "--set", "value", "edited"])
            .unwrap()
            .unwrap();
        assert_eq!(value, "edited");
    }
}

#[test]
fn configured_gateway_name_preserves_fixed_inner_spellings() {
    let value = Setup::new("test")
        .gateway(Gateway::new().long("inspect").short('I'))
        .into_bundle(|input: Input| input.value)
        .run_from(["test", "--inspect", "--set", "value", "edited"])
        .unwrap()
        .unwrap();
    assert_eq!(value, "edited");

    let bundle = Setup::new("test")
        .gateway(Gateway::new().long("inspect").short('I'))
        .into_bundle(|input: Input| input);
    assert!(bundle.command().clone().try_get_matches_from(["test", "--scry"]).is_err());
}

#[test]
fn disabling_default_short_alias_frees_it_for_an_application_option() {
    let bundle = Setup::new("test")
        .gateway(Gateway::new().short(None))
        .expose(|expose| {
            expose.option("value").short('S');
        })
        .into_bundle(|input: Input| input.value);
    let help = bundle.command().clone().render_help().to_string();
    assert!(help.contains("-S, --value"), "{help}");
    assert!(help.contains("--scry"), "{help}");
    let value = bundle
        .run_from([
            "test",
            "-S",
            "application",
            "--scry",
            "--set",
            "value",
            "edited",
        ])
        .unwrap()
        .unwrap();
    assert_eq!(value, "edited");
}

#[test]
fn application_get_and_set_options_remain_available_before_the_gateway() {
    let input = Setup::standard("test")
        .no_config_source()
        .expose(|expose| {
            expose.option("get");
            expose.option("set");
        })
        .into_bundle(|input: ApplicationNames| input)
        .run_from([
            "test",
            "--get",
            "application-get",
            "--set",
            "application-set",
            "--scry",
            "--set",
            "get",
            "toolbox-edit",
        ])
        .unwrap()
        .unwrap();

    assert_eq!(input.get, "toolbox-edit");
    assert_eq!(input.set, "application-set");
}

#[test]
fn setup_includes_the_gateway_without_enabling_it() {
    let value = Setup::new("test")
        .expose(|expose| {
            expose.option("value");
        })
        .into_bundle(|input: Input| input.value)
        .run_from([
            "test", "--value", "parent", "--scry", "--set", "value", "edited",
        ])
        .unwrap()
        .unwrap();

    assert_eq!(value, "edited");
}

#[test]
fn nested_toolbox_diagnostics_keep_the_configured_binary_and_command_path() {
    let leaf = Setup::new("operation")
        .gateway(Gateway::new().long("inspect"))
        .into_bundle(|_: Input| panic!("invalid toolbox input must not execute"));
    let bundle = Bundle::group_with(
        Command::new("application").bin_name("configured-app"),
        vec![Bundle::group("group", "Groups operations.", vec![leaf])],
    );
    let error = bundle
        .run_from([
            "ignored-argv-name",
            "group",
            "operation",
            "--inspect",
            "unknown",
        ])
        .unwrap_err();

    assert!(matches!(&error, SetupError::Gateway(_)), "{error}");
    assert!(error.to_string().contains("configured-app group operation --inspect"), "{error}");
}

#[test]
fn gateway_profile_has_no_parent_shortcuts_or_word_subcommands() {
    let bundle = Setup::standard("test")
        .no_config_source()
        .expose(|expose| {
            expose.positional("value");
        })
        .into_bundle(|input: Input| input);

    assert_eq!(bundle.command().get_subcommands().count(), 0);
    for flag in [
        "--get",
        "--desc",
        "--get-flat",
        "--get-as",
        "--set",
        "--remove",
    ] {
        assert!(bundle.command().clone().try_get_matches_from(["test", flag]).is_err());
    }
    for value in ["scry", "config"] {
        let input = Setup::new("test")
            .expose(|expose| {
                expose.positional("value");
            })
            .into_bundle(|input: Input| input)
            .run_from(["test", value])
            .unwrap()
            .unwrap();
        assert_eq!(input.value, value);
    }
}

#[test]
fn parent_end_of_options_keeps_gateway_spelling_literal() {
    let input = Setup::new("test")
        .expose(|expose| {
            expose.positional("value");
        })
        .into_bundle(|input: Input| input)
        .run_from(["test", "--", "--scry"])
        .unwrap()
        .unwrap();

    assert_eq!(input.value, "--scry");
}

#[test]
fn positional_arrays_stop_at_the_gateway_and_keep_literal_words() {
    let input = Setup::new("test")
        .expose(|expose| {
            expose.option("value");
            expose.positional("items").array(1..);
        })
        .into_bundle(|input: Input| input)
        .run_from([
            "test", "--value", "ready", "scry", "config", "--scry", "--set", "items[0]", "edited",
        ])
        .unwrap()
        .unwrap();

    assert_eq!(input.value, "ready");
    assert_eq!(input.items, ["edited", "config"]);
}

#[test]
fn gateway_does_not_hide_incomplete_parent_argument_groups() {
    let bundle = Setup::new("test")
        .expose(|expose| {
            expose.option("rows").array(2).append();
        })
        .into_bundle(|input: Input| input);
    assert!(bundle
        .command()
        .clone()
        .try_get_matches_from(["test", "--rows", "first", "--scry", "get"])
        .is_err());
}

#[test]
fn parent_help_advertises_one_gateway_and_keeps_application_options() {
    let bundle = Setup::new("test")
        .config_source(|_| ConfigSource::new().option("config-file", Some('C'), "Loads input."))
        .expose(|expose| {
            expose.option("value");
        })
        .into_bundle(|input: Input| input);
    let help = bundle.command().clone().render_help().to_string();

    assert!(help.contains("--scry"), "{help}");
    assert_eq!(help.matches("--scry").count(), 1, "{help}");
    assert!(help.contains("--config-file"), "{help}");
    assert!(help.contains("--value"), "{help}");
    assert!(!help.contains("--get"), "{help}");
    assert!(!help.contains("--set"), "{help}");
    assert!(!help.contains("Config options:"), "{help}");
}

#[test]
fn gateway_collisions_with_exposed_and_builtin_names_fail_construction() {
    let cases: &[fn() -> Setup] = &[
        || {
            Setup::new("test").expose(|expose| {
                expose.option("value").long("scry");
            })
        },
        || {
            Setup::new("test").expose(|expose| {
                expose.option("value").short('S');
            })
        },
        || Setup::new("test").gateway(Gateway::new().short('h')),
        || Setup::new("test").gateway(Gateway::new().long("help")),
    ];
    for (index, setup) in cases.iter().enumerate() {
        let result = std::panic::catch_unwind(|| {
            setup().into_bundle(|input: Input| input).command().clone().debug_assert();
        });
        assert!(result.is_err(), "colliding declaration {index} was accepted");
    }
}

#[test]
fn required_raw_arguments_fail_construction() {
    let cases: &[fn() -> Setup] = &[
        || Setup::new("test").arg(Arg::new("input").required(true)),
        || Setup::new("test").arg(Arg::new("runtime").long("runtime").required(true)),
    ];
    for (index, setup) in cases.iter().enumerate() {
        let result = std::panic::catch_unwind(|| {
            setup().into_bundle(|input: Input| input);
        });
        assert!(result.is_err(), "unsupported declaration {index} was accepted");
    }
}

#[test]
fn optional_raw_arguments_are_available_to_the_payload_and_allow_inspection() {
    for suffix in [vec![], vec!["--scry", "get"]] {
        let mut arguments = vec!["test", "item", "--value", "chosen"];
        arguments.extend_from_slice(&suffix);
        let result = Setup::new("test")
            .arg(Arg::new("runtime-path").value_parser(scry::clap::value_parser!(PathBuf)))
            .expose(|expose| {
                expose.option("value");
            })
            .into_bundle_with_matches(|input: Input, matches| {
                assert_eq!(
                    matches.get_one::<PathBuf>("runtime-path").unwrap(),
                    &PathBuf::from("item")
                );
                input.value
            })
            .run_from(arguments)
            .unwrap();

        assert_eq!(
            result.as_deref(),
            if suffix.is_empty() {
                Some("chosen")
            } else {
                None
            }
        );
    }
}

#[test]
fn positional_source_requirements_are_checked_only_for_input_actions() {
    for (suffix, needs_input) in [
        (vec!["--scry", "desc"], false),
        (vec!["--scry", "get"], true),
        (vec![], true),
    ] {
        let mut arguments = vec!["test"];
        arguments.extend_from_slice(&suffix);
        let result = Setup::new("test")
            .config_source(|source| source.positional("CONFIG", "Loads input."))
            .into_bundle(|_: DecodeTrap| panic!("missing input must not execute"))
            .run_from(arguments);

        if needs_input {
            assert!(matches!(result, Err(SetupError::NoConfigFile { .. })));
        } else {
            assert!(result.unwrap().is_none());
        }
    }
}

// ---------------------------------------------------------------------------------------------- //

fn loaded_setup(json: &'static str, loads: Rc<Cell<usize>>) -> Setup {
    Setup::new("test").config_source(|_| {
        ConfigSource::new().option("config-file", Some('C'), "Loads input.").loader(
            move |_: &Path| -> Result<Node, Infallible> {
                loads.set(loads.get() + 1);
                Ok(Node::parse_str(json, Format::Json).unwrap())
            },
        )
    })
}

fn unloaded_setup() -> Setup {
    Setup::new("test").config_source(|_| {
        ConfigSource::new()
            .option("config-file", Some('C'), "Loads input.")
            .discover(|_| -> Result<Option<PathBuf>, Infallible> {
                panic!("this request must not resolve a source");
            })
            .loader(|_: &Path| -> Result<Node, Infallible> {
                panic!("this request must not load a source");
            })
    })
}
