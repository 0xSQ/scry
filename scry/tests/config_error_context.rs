//! Preserves selected input context across typed configuration failures.

use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::num::ParseIntError;
use std::path::Path;

use scry::cli::setup::{ApplicationConfig, CommandConfigSource, Setup, SetupError};
use scry::cli::Bundle;
use scry::node::Format;
use scry::util::PathExt;
use scry::{Config, KeyPath, Node, NodeError};
use tempfile::tempdir;

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config)]
struct Input {
    /// Supplies nested command settings.
    nested: NestedInput,
}

#[derive(Debug, Config)]
struct NestedInput {
    /// Supplies a numeric value.
    value: u16,
}

#[test]
fn explicit_inputs_retain_whole_file_and_subtree_context_for_execution_and_check() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("input.rhai");
    for (selection, document) in [
        ("", "#{ nested: #{ value: \"invalid\" } }"),
        ("commands.render", "#{ commands: #{ render: #{ nested: #{ value: \"invalid\" } } } }"),
    ] {
        fs::write(&path, document).unwrap();
        for check in [false, true] {
            let mut arguments = vec![
                OsString::from("launcher"),
                "-C".into(),
                selected(&path, selection),
            ];
            if check {
                arguments.extend(["-S".into(), "check".into()]);
            }
            let error = Setup::standard("render")
                .into_bundle(reject_payload)
                .run_from(arguments)
                .unwrap_err();

            assert_selection(&error, "render", &path, selection);
            assert_numeric_cause(&error, "nested.value");
        }
    }
}

#[test]
fn nested_discovery_reports_the_selected_command_and_delegated_payload() {
    let directory = tempdir().unwrap();
    let application_dir = directory.path().join("config");
    let delegated_dir = directory.path().join("commands");
    fs::create_dir(&application_dir).unwrap();
    fs::create_dir(&delegated_dir).unwrap();

    for delegated in [false, true] {
        let path = if delegated {
            fs::write(application_dir.join("source.rhai"), "#{ directory: \"../commands\" }")
                .unwrap();
            fs::write(application_dir.join("atlas.rhai"), "throw \"local input must not load\";")
                .unwrap();
            application_dir.canonicalize_path().unwrap().join("../commands").join("atlas.rhai")
        } else {
            application_dir.join("atlas.rhai")
        };
        fs::write(&path, "#{ image: #{ render: #{ nested: #{ value: \"invalid\" } } } }").unwrap();
        let application = ApplicationConfig::new("atlas").directory(&application_dir);
        let render = Setup::standard("render")
            .config_source(|source| source.application(application))
            .into_bundle(reject_payload);
        let bundle = Bundle::group(
            "atlas",
            "Processes inputs.",
            vec![Bundle::group("image", "Processes images.", vec![render])],
        );
        let error =
            bundle.run_from(["renamed-program", "image", "render", "-S", "check"]).unwrap_err();

        assert_selection(&error, "atlas image render", &path, "image.render");
        assert_numeric_cause(&error, "nested.value");
        if delegated {
            let display = error.to_string();
            let physical = delegated_dir.join("atlas.rhai").canonicalize_path().unwrap();
            assert_ne!(path, physical);
            assert!(!display.contains(&physical.display().to_string()), "{display}");
            assert!(!display.contains("source.rhai"), "{display}");
        }
    }
}

#[test]
fn cli_failures_retain_base_selection_without_claiming_value_provenance() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("input.rhai");
    fs::write(&path, "#{ commands: #{ render: #{ nested: #{ value: 12 } } } }").unwrap();

    for edits in [
        vec!["--value", "invalid"],
        vec!["-S", "--set", "nested.value", "invalid", "check"],
    ] {
        let mut arguments = vec![
            OsString::from("render"),
            "-C".into(),
            selected(&path, "commands.render"),
        ];
        arguments.extend(edits.into_iter().map(OsString::from));
        let error = Setup::standard("render")
            .expose(|fields| {
                fields.option("nested.value").long("value");
            })
            .into_bundle(reject_payload)
            .run_from(arguments)
            .unwrap_err();

        assert_selection(&error, "render", &path, "commands.render");
        assert_numeric_cause(&error, "nested.value");
    }
}

#[test]
fn empty_input_keeps_command_context_without_inventing_a_source() {
    for setup in [
        Setup::standard("render").no_config_source(),
        Setup::standard("render").config_source(|source| source.or_empty()),
    ] {
        let error = setup.into_bundle(reject_payload).run_from(["render"]).unwrap_err();
        let SetupError::EvaluateInputConfig {
            command,
            config_source,
            source,
        } = &error
        else {
            panic!("unexpected error: {error}");
        };
        assert_eq!(command, "render");
        assert!(config_source.is_none());
        assert_eq!(
            error.to_string(),
            "failed to evaluate input configuration for command 'render'"
        );
        let cause = source.downcast_ref::<NodeError>().unwrap();
        assert_eq!(cause.path(), Some(&KeyPath::from_keys(["nested"])));
        assert!(matches!(cause, NodeError::MissingRequired { .. }));
    }
}

#[test]
fn custom_loaders_keep_available_metadata_and_tolerate_its_absence() {
    let directory = tempdir().unwrap();
    for (filename, has_metadata) in [("input.rhai", true), ("input.custom", false)] {
        let path = directory.path().join(filename);
        fs::write(&path, "custom loader input").unwrap();
        let error = Setup::standard("render")
            .config_source(|source| {
                source.loader(|_| {
                    Node::parse_str(
                        "#{ selected: #{ nested: #{ value: \"invalid\" } } }",
                        Format::Rhai,
                    )
                })
            })
            .into_bundle(reject_payload)
            .run_from([
                OsString::from("render"),
                "-C".into(),
                selected(&path, "selected"),
            ])
            .unwrap_err();

        assert_numeric_cause(&error, "nested.value");
        if has_metadata {
            assert_selection(&error, "render", &path, "selected");
        } else {
            let SetupError::EvaluateInputConfig {
                command,
                config_source,
                ..
            } = &error
            else {
                panic!("unexpected error: {error}");
            };
            assert_eq!(command, "render");
            assert!(config_source.is_none());
            assert_eq!(
                error.to_string(),
                "failed to evaluate input configuration for command 'render'"
            );
        }
    }
}

#[test]
fn reporters_preserve_nested_paths_and_causes_without_repeating_selection_context() {
    #[derive(Debug, Config)]
    struct IndexedInput {
        /// Supplies entries under a literal dotted key.
        #[scry(rename = "work.items")]
        entries: Vec<NestedInput>,
    }

    let directory = tempdir().unwrap();
    let path = directory.path().join("input.rhai");
    fs::write(&path, "#{ selected: #{ \"work.items\": [#{ value: \"invalid\" }] } }").unwrap();
    let error = Setup::standard("render")
        .into_bundle(|input: IndexedInput| {
            panic!("invalid input must not execute with {} entries", input.entries.len());
        })
        .run_from([
            OsString::from("render"),
            "-C".into(),
            selected(&path, "selected"),
        ])
        .unwrap_err();
    assert_selection(&error, "render", &path, "selected");
    assert_numeric_cause(&error, r#"["work.items"][0].value"#);

    let display = error.to_string();
    let debug = format!("{error:?}");
    let report = anyhow::Error::new(error).context("cannot prepare the selected command");
    assert!(report.downcast_ref::<SetupError>().is_some());
    assert!(report.chain().any(|cause| cause.downcast_ref::<NodeError>().is_some()));
    assert!(report.chain().any(|cause| cause.downcast_ref::<ParseIntError>().is_some()));
    for rendered in [debug, format!("{report:?}"), format!("{report:#}")] {
        assert_eq!(rendered.matches(&display).count(), 1, "{rendered}");
        assert_eq!(rendered.matches("selected from").count(), 1, "{rendered}");
        assert!(rendered.contains(r#"["work.items"][0].value"#), "{rendered}");
        assert!(rendered.contains("invalid digit found in string"), "{rendered}");
    }
}

fn reject_payload(input: Input) {
    panic!("invalid input must not execute with {}", input.nested.value);
}

fn selected(path: &Path, selection: &str) -> OsString {
    let mut argument = path.as_os_str().to_owned();
    if !selection.is_empty() {
        argument.push("::");
        argument.push(selection);
    }
    argument
}

fn assert_selection(error: &SetupError, expected_command: &str, path: &Path, selection: &str) {
    let SetupError::EvaluateInputConfig {
        command,
        config_source,
        ..
    } = error
    else {
        panic!("unexpected error: {error}");
    };
    assert_eq!(command, expected_command);
    assert_eq!(
        config_source.as_deref(),
        Some(&CommandConfigSource {
            logical_path: path.to_owned(),
            physical_path: path.canonicalize_path().unwrap(),
            selection: if selection.is_empty() {
                KeyPath::new()
            } else {
                selection.parse().unwrap()
            },
            format: Format::Rhai,
        })
    );
    let selection_suffix = if selection.is_empty() {
        format!(" (selected from '{}')", path.display())
    } else {
        format!(" (selected from '{}' at '{selection}')", path.display())
    };
    assert_eq!(
        error.to_string(),
        format!("failed to evaluate input configuration for command '{expected_command}'{selection_suffix}")
    );
}

fn assert_numeric_cause(error: &SetupError, expected_path: &str) {
    let cause = error.source().unwrap().downcast_ref::<NodeError>().unwrap();
    assert_eq!(cause.path(), Some(&expected_path.parse().unwrap()));
    assert!(cause.source().unwrap().downcast_ref::<ParseIntError>().is_some());
}
