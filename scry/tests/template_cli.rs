//! Integration tests for configuration templates in Setup commands.

use std::convert::Infallible;
use std::path::{Path, PathBuf};

use clap::error::ErrorKind;
use scry::cli::setup::{ConfigSource, Setup, SetupError, Support};
use scry::node::NodeError;
use scry::{Config, Desc, Describe, FromNode, Node};

// ---------------------------------------------------------------------------------------------- //

#[derive(Config)]
#[allow(dead_code)]
struct Input {
    /// Selects the input value.
    value: String,
    /// Limits repeated attempts.
    #[scry(default = 3)]
    retries: u16,
}

struct DecodeTrap;

impl FromNode for DecodeTrap {
    fn from_node(_: &Node) -> Result<Self, NodeError> {
        panic!("template generation must not decode application input");
    }
}

impl Describe for DecodeTrap {
    fn describe() -> Desc {
        Input::describe()
    }
}

#[test]
fn templates_skip_source_discovery_loading_decoding_and_payload() {
    let mut formats = vec![None, Some("rhai")];
    if cfg!(feature = "format-json5") {
        formats.push(Some("json5"));
    }
    for format in formats {
        for mode in [None, Some("min"), Some("ref"), Some("explicit")] {
            for source in [vec![], vec!["-C", "missing-config.rhai"]] {
                let mut arguments = vec!["demo"];
                arguments.extend(source);
                arguments.extend(["-S", "template"]);
                if let Some(format) = format {
                    arguments.extend(["--as", format]);
                }
                if let Some(mode) = mode {
                    arguments.extend(["--mode", mode]);
                }
                let result = unloaded_setup()
                    .into_bundle(|_: DecodeTrap| panic!("templates must not run the payload"))
                    .run_from(arguments)
                    .unwrap();
                assert!(result.is_none());
            }
        }
    }
}

#[test]
fn template_help_does_not_require_a_format_or_source() {
    let result = unloaded_setup()
        .into_bundle(|_: DecodeTrap| panic!("template help must not run the payload"))
        .run_from(["demo", "-S", "template", "--help"])
        .unwrap();
    assert!(result.is_none());
}

#[test]
fn template_formatting_options_skip_loading_decoding_and_payload() {
    for spacing in ["blank", "compact"] {
        for type_hints in ["above", "inline", "none"] {
            let result = unloaded_setup()
                .into_bundle(|_: DecodeTrap| panic!("templates must not run the payload"))
                .run_from([
                    "demo",
                    "-C",
                    "missing-config.rhai",
                    "-S",
                    "template",
                    "--mode",
                    "ref",
                    "--spacing",
                    spacing,
                    "--type-hints",
                    type_hints,
                ])
                .unwrap();
            assert!(result.is_none());
        }
    }
}

#[test]
fn template_parse_failures_skip_source_preparation() {
    for (suffix, kind) in [
        (vec!["template", "--as"], ErrorKind::InvalidValue),
        (vec!["template", "--as", "json"], ErrorKind::InvalidValue),
        (vec!["template", "--mode", "minimal"], ErrorKind::InvalidValue),
        (vec!["template", "--mode", "reference"], ErrorKind::InvalidValue),
        (vec!["template", "--spacing"], ErrorKind::InvalidValue),
        (vec!["template", "--spacing", "spaced"], ErrorKind::InvalidValue),
        (vec!["template", "--type-hints"], ErrorKind::InvalidValue),
        (vec!["template", "--type-hints", "off"], ErrorKind::InvalidValue),
        (vec!["--set", "value", "edited", "template"], ErrorKind::ArgumentConflict),
        (vec!["--remove", "value", "template"], ErrorKind::ArgumentConflict),
    ] {
        let mut arguments = vec!["demo", "-S"];
        arguments.extend(suffix);
        let result = unloaded_setup()
            .into_bundle(|_: DecodeTrap| panic!("invalid requests must not run the payload"))
            .run_from(arguments);
        let Err(SetupError::Support(error)) = result else {
            panic!("expected a support parse failure");
        };
        assert_eq!(error.kind(), kind);
    }
}

#[test]
fn configured_support_names_also_offer_templates() {
    for entrance in ["--inspect", "-I"] {
        let result = unloaded_setup()
            .support(Support::new().long("inspect").short('I'))
            .into_bundle(|_: DecodeTrap| panic!("templates must not run the payload"))
            .run_from(["demo", entrance, "template"])
            .unwrap();
        assert!(result.is_none());
    }
}

fn unloaded_setup() -> Setup {
    Setup::new("demo").config_source(|_| {
        ConfigSource::new()
            .option("config", Some('C'), "Loads input.")
            .discover(|_| -> Result<Option<PathBuf>, Infallible> {
                panic!("template requests must not resolve a source");
            })
            .loader(|_: &Path| -> Result<Node, Infallible> {
                panic!("template requests must not load a source");
            })
    })
}
