//! Exercises conventional command discovery through the public application and Setup APIs.

use std::cell::Cell;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use scry::clap::Command;
use scry::cli::setup::{
    ApplicationConfig, CommandConfigError, LoadedCommandConfig, MultiSourcePolicy, Setup,
    SetupError,
};
use scry::cli::Bundle;
use scry::node::Format;
use scry::util::PathExt;
use scry::{Config, KeyPath, Node, NodeError};
use tempfile::tempdir;

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config)]
struct ValueConfig {
    /// Supplies the command's value.
    value: i64,
}

#[test]
fn deep_commands_resolve_from_each_conventional_ancestor() {
    let placements = [
        ("enhance/mask/split.rhai", "#{ value: 17 }", Vec::new()),
        ("enhance/mask.rhai", "#{ split: #{ value: 17 } }", vec!["split"]),
        ("enhance.rhai", "#{ mask: #{ split: #{ value: 17 } } }", vec!["mask", "split"]),
        (
            "atlas.rhai",
            "#{ enhance: #{ mask: #{ split: #{ value: 17 } } } }",
            vec!["enhance", "mask", "split"],
        ),
    ];
    for (filename, source, selection) in placements {
        let directory = tempdir().unwrap();
        let file = directory.path().join(filename);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, source).unwrap();
        let application = ApplicationConfig::new("atlas").directory(directory.path());
        let loaded = split_invocation().load(&application).unwrap().unwrap();
        assert_eq!(loaded.node.req::<i64>("value").unwrap(), 17, "{filename}");
        assert_eq!(loaded.source.logical_path, file);
        assert_eq!(loaded.source.selection, KeyPath::from_keys(selection));
        assert!(loaded.node.path.is_empty());
        assert_eq!(loaded.node.req_node("value").unwrap().path.to_string(), "value");
    }
}

#[test]
fn application_missing_policy_distinguishes_absence_from_invalid_input() {
    let directory = tempdir().unwrap();
    let application = ApplicationConfig::new("atlas").directory(directory.path());
    let setup = || {
        Setup::standard("render")
            .config_source(|source| source.application(application.clone()))
            .expose(|fields| {
                fields.option("value");
            })
    };
    let error =
        setup().into_bundle(|input: ValueConfig| input.value).run_from(["render"]).unwrap_err();
    assert!(matches!(error, SetupError::NoCommandConfig { .. }));

    let value = setup()
        .config_source(|source| source.or_empty())
        .into_bundle_with_source(|input: ValueConfig, _, source| {
            assert!(source.is_none());
            input.value
        })
        .run_from(["render", "--value", "23"])
        .unwrap();
    assert_eq!(value, Some(23));

    fs::write(directory.path().join("atlas.rhai"), "#{ value:").unwrap();
    let error = setup()
        .config_source(|source| source.or_empty())
        .into_bundle(|input: ValueConfig| input.value)
        .run_from(["render", "--value", "23"])
        .unwrap_err();
    assert!(matches!(error, SetupError::CommandConfig(CommandConfigError::Load { .. })));
}

#[test]
fn enabled_formats_select_namespace_payloads_and_reject_unknown_children() {
    let cases = [
        ("rhai", "#{ \"$self\": #{ value: 3 }, canon: #{ value: 5 } }", Format::Rhai),
        #[cfg(feature = "format-json")]
        ("json", r#"{ "$self": { "value": 3 }, "canon": { "value": 5 } }"#, Format::Json),
        #[cfg(feature = "format-json5")]
        ("json5", "{ '$self': { value: 3 }, canon: { value: 5 } }", Format::Json5),
        #[cfg(feature = "format-toml")]
        ("toml", "[\"$self\"]\nvalue = 3\n[canon]\nvalue = 5\n", Format::Toml),
        #[cfg(feature = "format-yaml")]
        ("yaml", "\"$self\":\n  value: 3\ncanon:\n  value: 5\n", Format::Yaml),
    ];
    for (extension, source, format) in cases {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join(format!("stamp.{extension}")), source).unwrap();
        let application = ApplicationConfig::new("atlas").directory(directory.path());
        let root = Command::new("atlas")
            .subcommand(Command::new("stamp").subcommand(Command::new("canon")));
        for (path, expected, selection) in [
            (vec!["stamp"], 3, "$self"),
            (vec!["stamp", "canon"], 5, "canon"),
        ] {
            let loaded = Invocation::new(root.clone(), &path).load(&application).unwrap().unwrap();
            assert_eq!(loaded.node.req::<i64>("value").unwrap(), expected, "{extension}");
            assert_eq!(loaded.source.selection, KeyPath::from_keys([selection]));
            assert_eq!(loaded.source.format, format);
        }
        let unknown_child = Invocation::new(
            Command::new("atlas")
                .subcommand(Command::new("stamp").subcommand(Command::new("other"))),
            &["stamp"],
        );
        let error = unknown_child.load(&application).err().unwrap();
        assert!(matches!(error, CommandConfigError::Namespace { .. }), "{extension}: {error}");
    }
}

#[test]
fn computed_leaf_values_evaluate_once_and_keep_payload_fields_ordinary() {
    let directory = tempdir().unwrap();
    fs::write(
        directory.path().join("render.rhai"),
        "fn make() { #{ value: tick(), commands: #{ config: true }, \"$self\": 8 } } make()",
    )
    .unwrap();
    let calls = Rc::new(Cell::new(0));
    let application = counted_application(directory.path(), calls.clone());
    let loaded = leaf_invocation("render").load(&application).unwrap().unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(loaded.node.req::<i64>("value").unwrap(), 41);
    assert_eq!(loaded.node.req::<i64>(r#"["$self"]"#).unwrap(), 8);
    assert!(loaded.node.req::<bool>("commands.config").unwrap());
}

#[test]
fn mixed_siblings_evaluate_non_owning_ancestors_once() {
    let directory = tempdir().unwrap();
    fs::create_dir(directory.path().join("enhance")).unwrap();
    fs::create_dir(directory.path().join("enhance/mask")).unwrap();
    fs::write(directory.path().join("enhance/mask/split.rhai"), "#{ value: tick() }").unwrap();
    fs::write(
        directory.path().join("enhance.rhai"),
        indoc::indoc! {r#"
            fn callback() { 0 }
            let namespace = #{};
            namespace.inpaint = #{ value: tick(), callback: Fn("callback") };
            namespace
        "#},
    )
    .unwrap();
    let calls = Rc::new(Cell::new(0));
    let application = counted_application(directory.path(), calls.clone());
    let loaded = split_invocation().load(&application).unwrap().unwrap();
    assert_eq!(loaded.node.req::<i64>("value").unwrap(), 41);
    assert_eq!(calls.get(), 2);
}

#[test]
fn ambiguity_keeps_both_logical_definitions_after_eager_evaluation() {
    let directory = tempdir().unwrap();
    fs::create_dir_all(directory.path().join("enhance/mask")).unwrap();
    fs::write(directory.path().join("enhance/mask/split.rhai"), "#{ value: tick() }").unwrap();
    fs::write(directory.path().join("enhance.rhai"), "#{ mask: #{ split: #{ value: tick() } } }")
        .unwrap();
    fs::write(
        directory.path().join("atlas.rhai"),
        indoc::indoc! {r#"
            if env.ready {
                #{ enhance: #{ inpaint: #{ value: tick() } } }
            } else {
                throw "the environment was not ready";
            }
        "#},
    )
    .unwrap();
    fs::write(directory.path().join("env.rhai"), "#{ ready: true }").unwrap();
    let calls = Rc::new(Cell::new(0));
    let application = counted_application(directory.path(), calls.clone());
    let error = split_invocation().load(&application).err().unwrap();
    let CommandConfigError::Ambiguous { sources, .. } = error else {
        panic!("expected both definitions to be reported");
    };
    assert_eq!(sources.len(), 2);
    assert!(sources.iter().any(|source| {
        source.logical_path == directory.path().join("enhance/mask/split.rhai")
            && source.selection.is_empty()
    }));
    assert!(sources.iter().any(|source| {
        source.logical_path == directory.path().join("enhance.rhai")
            && source.selection == KeyPath::from_keys(["mask", "split"])
    }));
    assert_eq!(calls.get(), 3);
}

#[test]
fn imported_computed_namespaces_use_the_application_engine_once() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("enhance.rhai");
    fs::write(
        &path,
        indoc::indoc! {r#"
            import "helpers.rhai" as helpers;
            helpers::namespace
        "#},
    )
    .unwrap();
    fs::write(
        directory.path().join("helpers.rhai"),
        indoc::indoc! {r#"
            fn make_namespace() { #{ mask: #{ split: #{ value: tick() } } } }
            export let namespace = make_namespace();
        "#},
    )
    .unwrap();
    let calls = Rc::new(Cell::new(0));
    let application = counted_application(directory.path(), calls.clone());
    let loaded = split_invocation().load(&application).unwrap().unwrap();
    assert_eq!(loaded.node.req::<i64>("value").unwrap(), 41);
    assert_eq!(loaded.source.logical_path, path);
    assert_eq!(loaded.source.selection, KeyPath::from_keys(["mask", "split"]));
    assert_eq!(calls.get(), 1);
}

#[test]
fn selected_payload_is_converted_without_decoding_unrelated_dynamic_values() {
    let directory = tempdir().unwrap();
    fs::write(
        directory.path().join("atlas.rhai"),
        indoc::indoc! {r#"
            fn callback() { 0 }
            let namespace = #{ render: #{ value: 7 } };
            namespace.other = Fn("callback");
            namespace
        "#},
    )
    .unwrap();
    let invocation = Invocation::new(
        Command::new("atlas")
            .subcommand_required(true)
            .subcommands([Command::new("render"), Command::new("other")]),
        &["render"],
    );
    let application = ApplicationConfig::new("atlas").directory(directory.path());
    let loaded = invocation.load(&application).unwrap().unwrap();
    assert_eq!(loaded.node.req::<i64>("value").unwrap(), 7);
}

#[test]
fn executable_namespaces_select_self_and_children_without_inheritance() {
    let directory = tempdir().unwrap();
    fs::write(
        directory.path().join("stamp.rhai"),
        "#{ \"$self\": #{ value: 3 }, canon: #{ value: 5 }, ux: #{ value: 7 } }",
    )
    .unwrap();
    let application = ApplicationConfig::new("atlas").directory(directory.path());
    let root = Command::new("atlas")
        .subcommand(Command::new("stamp").subcommands([Command::new("canon"), Command::new("ux")]));
    for (path, expected, selection) in [
        (vec!["stamp"], 3, vec!["$self"]),
        (vec!["stamp", "canon"], 5, vec!["canon"]),
        (vec!["stamp", "ux"], 7, vec!["ux"]),
    ] {
        let invocation = Invocation::new(root.clone(), &path);
        let loaded = invocation.load(&application).unwrap().unwrap();
        assert_eq!(loaded.node.req::<i64>("value").unwrap(), expected);
        assert_eq!(loaded.source.selection, KeyPath::from_keys(selection));
    }
    let pure_group =
        root.clone().mut_subcommand("stamp", |command| command.subcommand_required(true));
    let invocation = Invocation::new(pure_group, &["stamp", "canon"]);
    let error = invocation.load(&application).err().unwrap();
    assert!(matches!(error, CommandConfigError::Namespace { .. }));
    fs::write(directory.path().join("stamp.rhai"), "#{ canon: #{ value: 5 } }").unwrap();
    let invocation = Invocation::new(root.clone(), &["stamp"]);
    assert!(invocation.load(&application).unwrap().is_none());
    fs::write(directory.path().join("atlas.rhai"), "#{ \"$self\": #{ value: 11 } }").unwrap();
    let invocation = Invocation::new(root, &[]);
    let loaded = invocation.load(&application).unwrap().unwrap();
    assert_eq!(loaded.node.req::<i64>("value").unwrap(), 11);
}

#[test]
fn returned_namespaces_reject_unknown_keys_and_self_on_pure_groups() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("enhance.rhai");
    let application = ApplicationConfig::new("atlas").directory(directory.path());
    for source in [
        "#{ typo: #{ value: 1 } }",
        "#{ \"$self\": #{ value: 1 } }",
        "let namespace = #{ mask: #{ typo: #{ value: 1 } } }; namespace",
        "fn namespace() { #{ mask: #{ \"$self\": #{ value: 1 } } } } namespace()",
        "let namespace = []; namespace",
    ] {
        fs::write(&path, source).unwrap();
        let error = split_invocation().load(&application).err().unwrap();
        assert!(matches!(error, CommandConfigError::Namespace { .. }), "{source}: {error}");
    }
}

#[test]
fn direct_clap_trees_reject_reserved_duplicate_and_traversal_child_names() {
    let directory = tempdir().unwrap();
    let application = ApplicationConfig::new("atlas").directory(directory.path());
    for invalid_children in [
        vec![Command::new("render"), Command::new("$self")],
        vec![Command::new("render"), Command::new("render")],
        vec![Command::new("render"), Command::new("../escape")],
    ] {
        let invocation = Invocation::new(
            Command::new("atlas").subcommand_required(true).subcommands(invalid_children),
            &["render"],
        );
        let error = invocation.load(&application).err().unwrap();
        assert!(matches!(error, CommandConfigError::InvalidCommandTree { .. }));
    }
}

#[test]
fn malformed_consulted_ancestors_fail_even_when_a_separate_leaf_exists() {
    let directory = tempdir().unwrap();
    fs::create_dir_all(directory.path().join("enhance/mask")).unwrap();
    fs::write(directory.path().join("enhance/mask/split.rhai"), "#{ value: 17 }").unwrap();
    fs::write(directory.path().join("enhance.rhai"), "#{ inpaint: #{ value: 1 }").unwrap();
    let application = ApplicationConfig::new("atlas").directory(directory.path());
    let error = split_invocation().load(&application).err().unwrap();
    assert!(
        matches!(error, CommandConfigError::Load { path, .. } if path == directory.path().join("enhance.rhai"))
    );
}

#[test]
fn runtime_errors_from_non_owning_ancestors_are_reported() {
    let directory = tempdir().unwrap();
    fs::create_dir_all(directory.path().join("enhance/mask")).unwrap();
    fs::write(directory.path().join("enhance/mask/split.rhai"), "#{ value: 17 }").unwrap();
    let path = directory.path().join("enhance.rhai");
    fs::write(
        &path,
        indoc::indoc! {r#"
            fn payload() { throw "non-owning ancestor failed"; }
            #{ inpaint: payload() }
        "#},
    )
    .unwrap();
    let application = ApplicationConfig::new("atlas").directory(directory.path());
    let error = split_invocation().load(&application).err().unwrap();
    let CommandConfigError::Load {
        path: failed_path,
        source,
    } = error
    else {
        panic!("expected the ancestor's runtime error");
    };
    assert_eq!(failed_path, path);
    let mut cause: Option<&dyn std::error::Error> = Some(source.as_ref());
    let mut messages = Vec::new();
    while let Some(error) = cause {
        messages.push(error.to_string());
        cause = error.source();
    }
    assert!(
        messages.iter().any(|message| message.contains("non-owning ancestor failed")),
        "{messages:?}"
    );
}

#[test]
fn explicit_selectors_are_reanchored_before_cli_overrides_and_support_edits() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("any-name.rhai");
    fs::write(&path, "#{ outer: #{ \"$self\": #{ value: 2 } } }").unwrap();
    let application = ApplicationConfig::new("atlas").directory(directory.path());
    let setup = Setup::standard("render")
        .config_source(|source| source.application(application))
        .expose(|fields| {
            fields.option("value");
        })
        .into_bundle_with_source(|input: ValueConfig, _, source| {
            (input.value, source.cloned().unwrap())
        });
    let arguments = vec![
        OsString::from("render"),
        "-C".into(),
        selected_file(&path, "outer[\"$self\"]"),
        "--value".into(),
        "5".into(),
        "-S".into(),
        "--set".into(),
        "value".into(),
        "9".into(),
    ];
    let (value, source) = setup.run_from(arguments).unwrap().unwrap();
    assert_eq!(value, 9);
    assert_eq!(source.selection, KeyPath::from_keys(["outer", "$self"]));
    assert_eq!(source.logical_path, path);
    assert_eq!(source.physical_path, path.canonicalize_path().unwrap());
}

#[test]
fn ordinary_explicit_selectors_do_not_convert_unused_rhai_values() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("values.rhai");
    fs::write(&path, "fn callback() { 0 } #{ chosen: #{ value: 5 }, other: Fn(\"callback\") }")
        .unwrap();
    let value = Setup::standard("render")
        .into_bundle(|input: ValueConfig| input.value)
        .run_from([
            OsString::from("render"),
            "-C".into(),
            selected_file(&path, "chosen"),
        ])
        .unwrap()
        .unwrap();
    assert_eq!(value, 5);
}

#[test]
fn bare_explicit_namespace_filenames_supply_the_whole_payload() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("atlas.rhai");
    fs::write(&path, "#{ value: 43 }").unwrap();
    let application = ApplicationConfig::new("atlas").directory(directory.path());
    let (value, source) = Setup::standard("render")
        .config_source(|source| source.application(application))
        .into_bundle_with_source(|input: ValueConfig, _, source| {
            (input.value, source.cloned().unwrap())
        })
        .run_from(["render", "-C", path.to_str().unwrap()])
        .unwrap()
        .unwrap();
    assert_eq!(value, 43);
    assert_eq!(source.logical_path, path);
    assert!(source.selection.is_empty());
}

#[test]
fn explicit_selectors_preserve_quoted_keys_delimiters_and_array_indices() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("values.rhai");
    fs::write(
        &path,
        r#"#{ "odd-key::with.dot": [#{ value: 1 }, #{ "payload-key": #{ value: 37 } }] }"#,
    )
    .unwrap();
    let application = ApplicationConfig::new("atlas").directory(directory.path());
    let (value, source) = Setup::standard("render")
        .config_source(|source| source.application(application))
        .into_bundle_with_source(|input: ValueConfig, _, source| {
            (input.value, source.cloned().unwrap())
        })
        .run_from([
            OsString::from("render"),
            "-C".into(),
            selected_file(&path, r#"["odd-key::with.dot"][1]["payload-key"]"#),
        ])
        .unwrap()
        .unwrap();
    assert_eq!(value, 37);
    assert_eq!(source.logical_path, path);
    assert_eq!(
        source.selection,
        KeyPath::from_keys(["odd-key::with.dot"]).push_index(1).push_key("payload-key")
    );
}

#[test]
fn positional_and_option_sources_accept_combined_file_selection() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("values.rhai");
    fs::write(&path, "#{ selected: #{ value: 47 } }").unwrap();
    for positional in [true, false] {
        let setup = if positional {
            Setup::new("render")
                .config_source(|source| source.positional("CONFIG", "Path to a config file."))
        } else {
            Setup::standard("render")
        };
        let mut arguments = vec![OsString::from("render")];
        if !positional {
            arguments.push("-C".into());
        }
        arguments.push(selected_file(&path, "selected"));
        let value = setup
            .into_bundle(|input: ValueConfig| input.value)
            .run_from(arguments)
            .unwrap()
            .unwrap();
        assert_eq!(value, 47);
    }
}

#[test]
fn empty_explicit_selectors_fail_before_loading() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("missing.rhai");
    let calls = Rc::new(Cell::new(0));
    let loader_calls = calls.clone();
    let result = Setup::standard("render")
        .config_source(|source| {
            source.loader(move |_| {
                loader_calls.set(loader_calls.get() + 1);
                Ok::<_, NodeError>(Node::empty_map())
            })
        })
        .into_bundle(|input: ValueConfig| input.value)
        .run_from([
            OsString::from("render"),
            "-C".into(),
            selected_file(&path, ""),
        ]);
    assert!(result.is_err());
    assert_eq!(calls.get(), 0);
}

#[test]
fn different_selectors_of_one_file_remain_multiple_explicit_sources() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("values.rhai");
    fs::write(&path, "#{ left: #{ value: 53 }, right: #{ value: 59 } }").unwrap();
    for (policy, expected) in [
        (MultiSourcePolicy::ErrorOnMultiple, None),
        (MultiSourcePolicy::PreferPositional, Some(53)),
        (MultiSourcePolicy::PreferOption, Some(59)),
    ] {
        let calls = Rc::new(Cell::new(0));
        let loader_calls = calls.clone();
        let result = Setup::standard("render")
            .config_source(|source| {
                source
                    .positional("CONFIG", "Path to a config file.")
                    .multi_source_policy(policy)
                    .loader(move |path| {
                        loader_calls.set(loader_calls.get() + 1);
                        Node::parse_file(path)
                    })
            })
            .into_bundle(|input: ValueConfig| input.value)
            .run_from([
                OsString::from("render"),
                selected_file(&path, "left"),
                "-C".into(),
                selected_file(&path, "right"),
            ]);
        if let Some(expected) = expected {
            assert_eq!(result.unwrap(), Some(expected));
            assert_eq!(calls.get(), 1);
        } else {
            assert!(matches!(result, Err(SetupError::MultipleConfigPaths)));
            assert_eq!(calls.get(), 0);
        }
    }
}

#[test]
fn source_precedence_does_not_parse_the_discarded_selector() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("values.rhai");
    fs::write(&path, "#{ selected: #{ value: 61 } }").unwrap();
    for policy in [
        MultiSourcePolicy::PreferPositional,
        MultiSourcePolicy::PreferOption,
    ] {
        let (positional, option) = if policy == MultiSourcePolicy::PreferPositional {
            (selected_file(&path, "selected"), selected_file(&path, ""))
        } else {
            (selected_file(&path, ""), selected_file(&path, "selected"))
        };
        let value = Setup::standard("render")
            .config_source(|source| {
                source.positional("CONFIG", "Path to a config file.").multi_source_policy(policy)
            })
            .into_bundle(|input: ValueConfig| input.value)
            .run_from([OsString::from("render"), positional, "-C".into(), option])
            .unwrap()
            .unwrap();
        assert_eq!(value, 61);
    }
}

#[test]
fn custom_discovery_paths_are_literal_even_when_they_contain_a_delimiter() {
    let value = Setup::new("render")
        .config_source(|source| {
            source.discover(|_| Ok::<_, NodeError>(Some("literal::name.rhai".into()))).loader(
                |path| {
                    assert_eq!(path, Path::new("literal::name.rhai"));
                    let mut node = Node::empty_map();
                    node.set_value("value", 67)?;
                    Ok::<_, NodeError>(node)
                },
            )
        })
        .into_bundle(|input: ValueConfig| input.value)
        .run_from(["render"])
        .unwrap()
        .unwrap();
    assert_eq!(value, 67);
}

#[test]
fn nested_bundle_groups_and_executable_parents_select_their_own_payloads() {
    let directory = tempdir().unwrap();
    fs::write(
        directory.path().join("atlas.rhai"),
        "#{ enhance: #{ mask: #{ \"$self\": #{ value: 11 }, split: #{ value: 13 } } } }",
    )
    .unwrap();
    for (arguments, expected) in [
        (vec!["atlas", "enhance", "mask"], 11),
        (vec!["atlas", "enhance", "mask", "split"], 13),
    ] {
        let application = ApplicationConfig::new("atlas").directory(directory.path());
        let split = Setup::standard("split")
            .config_source(|source| source.application(application.clone()))
            .into_bundle(|input: ValueConfig| input.value);
        let mask = Setup::standard("mask")
            .config_source(|source| source.application(application))
            .into_bundle(|input: ValueConfig| input.value)
            .with_subcommands(vec![split]);
        let bundle = Bundle::group(
            "atlas",
            "Application.",
            vec![Bundle::group("enhance", "Enhance.", vec![mask])],
        );
        assert_eq!(bundle.run_from(arguments).unwrap(), Some(expected));
    }
}

#[test]
fn descriptions_and_templates_skip_config_root_and_namespace_validation() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("root.rhai"), "throw \"root is unused\";").unwrap();
    for action in ["desc", "template"] {
        let application = ApplicationConfig::new("atlas").directory(directory.path());
        let bundle = Setup::standard("render")
            .config_source(|source| source.application(application))
            .into_bundle(|_: ValueConfig| -> i64 {
                panic!("support actions must not run the payload");
            })
            .with_subcommands(vec![Bundle::single(Command::new("$self"), |_| {
                panic!("parent support must not dispatch a child");
            })]);
        let help = bundle.command().clone().try_get_matches_from(["render", "--help"]).unwrap_err();
        assert_eq!(help.kind(), scry::clap::error::ErrorKind::DisplayHelp);
        let result = bundle.run_from(["render", "-C", "missing.rhai::", "-S", action]);
        assert!(result.unwrap().is_none());
    }
}

#[cfg(feature = "format-json")]
#[test]
fn explicit_data_files_bypass_a_broken_ambient_root() {
    let directory = tempdir().unwrap();
    let conventional = directory.path().join("config");
    fs::create_dir(&conventional).unwrap();
    fs::write(conventional.join("root.rhai"), "throw \"root is unused\";").unwrap();
    let path = directory.path().join("input.json");
    fs::write(&path, r#"{ "outer": { "value": 29 } }"#).unwrap();
    let application = ApplicationConfig::new("atlas").directory(conventional);
    let (value, source) = Setup::standard("render")
        .config_source(|source| source.application(application))
        .into_bundle_with_source(|input: ValueConfig, _, source| {
            (input.value, source.cloned().unwrap())
        })
        .run_from([
            OsString::from("render"),
            "-C".into(),
            selected_file(&path, "outer"),
        ])
        .unwrap()
        .unwrap();
    assert_eq!(value, 29);
    assert_eq!(source.logical_path, path);
    assert_eq!(source.selection, KeyPath::from_keys(["outer"]));
}

#[cfg(any(unix, windows))]
#[test]
fn physical_aliases_evaluate_once_but_keep_each_logical_definition() {
    let directory = tempdir().unwrap();
    let conventional = directory.path().join("config");
    fs::create_dir(&conventional).unwrap();
    let physical = directory.path().join("namespace.data");
    fs::write(&physical, "#{ render: #{ value: tick() } }").unwrap();
    let leaf_path = conventional.join("render.rhai");
    let app_path = conventional.join("atlas.rhai");
    if !create_file_link(&physical, &leaf_path) || !create_file_link(&physical, &app_path) {
        return;
    }
    let calls = Rc::new(Cell::new(0));
    let application = counted_application(&conventional, calls.clone());
    let error = leaf_invocation("render").load(&application).err().unwrap();
    let CommandConfigError::Ambiguous { sources, .. } = error else {
        panic!("expected both logical definitions of the shared file");
    };
    assert_eq!(calls.get(), 1);
    assert_eq!(sources.len(), 2);
    assert!(sources.iter().all(|source| {
        source.physical_path == physical.canonicalize_path().unwrap()
            && source.format == Format::Rhai
    }));
    assert!(sources
        .iter()
        .any(|source| { source.logical_path == leaf_path && source.selection.is_empty() }));
    assert!(sources.iter().any(|source| {
        source.logical_path == app_path && source.selection == KeyPath::from_keys(["render"])
    }));
}

#[cfg(any(unix, windows))]
#[test]
fn linked_leaf_files_keep_their_language_and_physical_import_context() {
    let directory = tempdir().unwrap();
    let conventional = directory.path().join("config");
    let physical = directory.path().join("physical");
    fs::create_dir(&conventional).unwrap();
    fs::create_dir(&physical).unwrap();
    fs::write(physical.join("helpers.rhai"), "export let value = 31;").unwrap();
    let target = physical.join("payload.data");
    fs::write(&target, "import \"helpers.rhai\" as helpers; #{ value: helpers::value }").unwrap();
    let logical = conventional.join("render.rhai");
    if !create_file_link(&target, &logical) {
        return;
    }
    let application = ApplicationConfig::new("atlas").directory(conventional);
    let loaded = leaf_invocation("render").load(&application).unwrap().unwrap();
    assert_eq!(loaded.node.req::<i64>("value").unwrap(), 31);
    assert_eq!(loaded.source.logical_path, logical);
    assert_eq!(loaded.source.physical_path, target.canonicalize_path().unwrap());
    assert_eq!(loaded.source.format, Format::Rhai);
}

fn counted_application(directory: &Path, calls: Rc<Cell<usize>>) -> ApplicationConfig {
    ApplicationConfig::new("atlas").directory(directory).rhai_engine(move |engine, _| {
        let calls = calls.clone();
        engine.register_fn("tick", move || {
            calls.set(calls.get() + 1);
            41_i64
        });
    })
}

fn selected_file(path: &Path, selection: &str) -> OsString {
    let mut input = path.as_os_str().to_owned();
    input.push("::");
    input.push(selection);
    input
}

struct Invocation {
    root: Command,
    path: Vec<String>,
}

impl Invocation {
    fn new(root: Command, path: &[&str]) -> Self {
        Self {
            root,
            path: path.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    fn load(
        &self,
        application: &ApplicationConfig,
    ) -> Result<Option<LoadedCommandConfig>, CommandConfigError> {
        application.load_command(&self.root, &self.path)
    }
}

fn leaf_invocation(name: &str) -> Invocation {
    Invocation::new(
        Command::new("atlas").subcommand_required(true).subcommand(Command::new(name.to_owned())),
        &[name],
    )
}

fn split_invocation() -> Invocation {
    Invocation::new(
        Command::new("atlas").subcommand_required(true).subcommand(
            Command::new("enhance").subcommand_required(true).subcommands([
                Command::new("inpaint"),
                Command::new("mask").subcommand_required(true).subcommand(Command::new("split")),
            ]),
        ),
        &["enhance", "mask", "split"],
    )
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
            if cfg!(windows)
                && (error.kind() == std::io::ErrorKind::PermissionDenied
                    || error.raw_os_error() == Some(1314)) =>
        {
            eprintln!("Skipping file-link test because this host does not permit symlinks.");
            false
        }
        Err(error) => panic!("failed to create file link: {error}"),
    }
}
