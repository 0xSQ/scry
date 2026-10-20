//! Covers the boundary between setup errors and payload return values.
#![cfg(feature = "format-json")]

use std::io;
use std::path::Path;

use scry::cli::setup::{Setup, SetupError};
use scry::{Config, Node};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config)]
struct TestConfig {
    value: String,
}

#[test]
fn handler_errors_remain_inside_the_payload_result() {
    let io_result = Setup::new("test")
        .into_bundle(|_: TestConfig| -> Result<(), io::Error> {
            Err(io::Error::other("handler failed"))
        })
        .run_from(["test", "-S", "--set", "value", "test"])
        .unwrap()
        .unwrap();
    assert_eq!(io_result.unwrap_err().to_string(), "handler failed");

    let anyhow_result = Setup::new("test")
        .into_bundle(|_: TestConfig| -> anyhow::Result<()> {
            Err(anyhow::anyhow!("anyhow handler failed"))
        })
        .run_from(["test", "-S", "--set", "value", "test"])
        .unwrap()
        .unwrap();
    assert_eq!(anyhow_result.unwrap_err().to_string(), "anyhow handler failed");
}

#[test]
fn loader_errors_preserve_the_path_and_original_cause() {
    let io_error = Setup::standard("test")
        .config_source(|source| {
            source.loader(|_: &Path| -> Result<Node, io::Error> {
                Err(io::Error::new(io::ErrorKind::NotFound, "input is missing"))
            })
        })
        .into_bundle(|_: TestConfig| panic!("failed loading must skip the payload"))
        .run_from(["test", "-C", "missing.json"])
        .unwrap_err();
    let SetupError::LoadConfigFile { path, source } = io_error else {
        panic!("expected a load error");
    };
    assert_eq!(path, Path::new("missing.json"));
    assert_eq!(source.downcast_ref::<io::Error>().unwrap().kind(), io::ErrorKind::NotFound);
    assert_eq!(source.to_string(), "input is missing");

    let anyhow_error = Setup::standard("test")
        .config_source(|source| {
            source.loader(|_: &Path| -> anyhow::Result<Node> {
                Err(anyhow::anyhow!("anyhow loading failed"))
            })
        })
        .into_bundle(|_: TestConfig| panic!("failed loading must skip the payload"))
        .run_from(["test", "-C", "config.json"])
        .unwrap_err();
    let SetupError::LoadConfigFile { path, source } = anyhow_error else {
        panic!("expected a load error");
    };
    assert_eq!(path, Path::new("config.json"));
    assert_eq!(source.to_string(), "anyhow loading failed");
}

#[test]
fn default_loader_reads_the_file_and_preserves_the_payload_type() {
    #[derive(Debug, PartialEq)]
    struct Output(String);

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.json");
    std::fs::write(&path, r#"{ "value": "from file" }"#).unwrap();
    let result = Setup::standard("test")
        .into_bundle(|config: TestConfig| Output(config.value))
        .run_from([Path::new("test"), Path::new("-C"), &path])
        .unwrap();

    assert_eq!(result, Some(Output("from file".to_owned())));
}
