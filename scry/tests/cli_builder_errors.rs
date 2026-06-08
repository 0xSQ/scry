//! Integration tests for CLI Setup error handling.
#![cfg(feature = "format-json")]

use std::io;
use std::path::Path;

use scry::cli::setup::{ConfigSource, OverrideArgs, QueryArgs, Required, Setup, SetupError};
use scry::node::Format;
use scry::{Config, Node};

// ---------------------------------------------------------------------------------------------- //
// Test Config Type

#[derive(Debug, Config)]
struct TestConfig {
    /// A test value.
    value: String,
}

// ---------------------------------------------------------------------------------------------- //
// Test 1: Handler returning Result has error in returned T

#[test]
fn handler_error_is_in_returned_type() {
    let bundle = Setup::new("test")
        .override_args(OverrideArgs::new())
        .query_args(QueryArgs::new())
        .config_source(|c| {
            c.positional("CONFIG", "Path to config file", Required::Yes).loader(
                |_path: &Path| -> Result<Node, std::convert::Infallible> {
                    Ok(Node::parse_str(r#"{"value": "test"}"#, Format::Json).unwrap())
                },
            )
        })
        .into_bundle(|_cfg: TestConfig| -> Result<(), io::Error> {
            Err(io::Error::other("handler failed"))
        });

    let result = bundle.run_from(["test", "config.json"]);

    // Execution succeeds (Ok), handler ran (Payload), handler returned error (Err)
    match result {
        Ok(Some(Err(ref io_err))) => {
            assert!(io_err.to_string().contains("handler failed"));
        }
        other => panic!("Expected Ok(Payload::Executed(Err(...))), got {:?}", other),
    }
}

// ---------------------------------------------------------------------------------------------- //
// Test 2: Infallible handler succeeds

#[test]
fn infallible_handler_succeeds() {
    let bundle = Setup::new("test")
        .override_args(OverrideArgs::new())
        .query_args(QueryArgs::new())
        .config_source(|c| {
            c.positional("CONFIG", "Path to config file", Required::Yes).loader(
                |_path: &Path| -> Result<Node, std::convert::Infallible> {
                    Ok(Node::parse_str(r#"{"value": "test"}"#, Format::Json).unwrap())
                },
            )
        })
        .into_bundle(|cfg: TestConfig| {
            // Handler returns () - infallible
            assert_eq!(cfg.value, "test");
        });

    let result = bundle.run_from(["test", "config.json"]);

    // Execution succeeds, handler ran, handler returned ()
    assert!(matches!(result, Ok(Some(()))));
}

// ---------------------------------------------------------------------------------------------- //
// Test 3: Loader error classification

#[test]
fn loader_error_is_config_error() {
    let bundle = Setup::new("test")
        .override_args(OverrideArgs::new())
        .query_args(QueryArgs::new())
        .config_source(|c| {
            c.positional("CONFIG", "Path to config file", Required::Yes).loader(
                |path: &Path| -> Result<Node, io::Error> {
                    Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("file not found: {}", path.display()),
                    ))
                },
            )
        })
        .into_bundle(|_cfg: TestConfig| {
            // Handler won't be reached
        });

    let result = bundle.run_from(["test", "missing.json"]);

    // Should be SetupError::LoadConfigFile with a load error message.
    match result {
        Err(SetupError::LoadConfigFile { path, source }) => {
            let msg = source.to_string();
            assert!(
                path.to_string_lossy().contains("missing.json"),
                "path should mention missing.json: {}",
                path.display()
            );
            assert!(msg.contains("missing.json"), "error should mention the path: {}", msg);
        }
        other => panic!("Expected Err(SetupError::LoadConfigFile(...)), got {:?}", other),
    }
}

// ---------------------------------------------------------------------------------------------- //
// Test 4: Default loader uses Node::parse_file

#[test]
fn default_loader_uses_parse_file() {
    // Create a temp file with valid config
    let temp_dir = std::env::temp_dir();
    let config_path = temp_dir.join("test_default_loader.json");
    std::fs::write(&config_path, r#"{"value": "from file"}"#).unwrap();

    let bundle = Setup::standard("test")
        .override_args(OverrideArgs::new())
        .query_args(QueryArgs::new())
        // No loader configured - should use default Node::parse_file
        .into_bundle(|cfg: TestConfig| {
            assert_eq!(cfg.value, "from file");
        });

    let result = bundle.run_from(["test", config_path.to_str().unwrap()]);

    // Clean up
    let _ = std::fs::remove_file(&config_path);

    assert!(matches!(result, Ok(Some(()))));
}

// ---------------------------------------------------------------------------------------------- //
// Test 5: anyhow::Result handler works

#[test]
fn anyhow_handler_works() {
    let bundle = Setup::new("test")
        .override_args(OverrideArgs::new())
        .query_args(QueryArgs::new())
        .config_source(|c| {
            c.positional("CONFIG", "Path to config file", Required::Yes).loader(
                |_path: &Path| -> Result<Node, std::convert::Infallible> {
                    Ok(Node::parse_str(r#"{"value": "test"}"#, Format::Json).unwrap())
                },
            )
        })
        .into_bundle(|_cfg: TestConfig| -> anyhow::Result<()> {
            anyhow::bail!("anyhow error message")
        });

    let result = bundle.run_from(["test", "config.json"]);

    // Execution succeeds, handler ran, handler returned anyhow error
    match result {
        Ok(Some(Err(ref anyhow_err))) => {
            assert!(anyhow_err.to_string().contains("anyhow error message"));
        }
        other => panic!("Expected Ok(Some(Err(...))), got {:?}", other),
    }
}

// ---------------------------------------------------------------------------------------------- //
// Test 6: anyhow loader works

#[test]
fn anyhow_loader_works() {
    let bundle = Setup::new("test")
        .override_args(OverrideArgs::new())
        .query_args(QueryArgs::new())
        .config_source(|c| {
            c.positional("CONFIG", "Path to config file", Required::Yes).loader(
                |path: &Path| -> anyhow::Result<Node> {
                    anyhow::bail!("failed to load: {}", path.display())
                },
            )
        })
        .into_bundle(|_cfg: TestConfig| {
            // Handler won't be reached
        });

    let result = bundle.run_from(["test", "config.json"]);

    // Should be SetupError::LoadConfigFile with a load error message.
    match result {
        Err(SetupError::LoadConfigFile { path, source }) => {
            let msg = source.to_string();
            assert!(
                path.to_string_lossy().contains("config.json"),
                "path should mention config.json: {}",
                path.display()
            );
            assert!(msg.contains("config.json"), "error should mention the path: {}", msg);
        }
        other => panic!("Expected Err(SetupError::LoadConfigFile(...)), got {:?}", other),
    }
}

// ---------------------------------------------------------------------------------------------- //
// Test 7: Collision panics

#[test]
#[should_panic(expected = "CLI argument collision")]
fn collision_panics() {
    use clap::Arg;

    // Create a deliberate collision - should panic
    let _bundle = Setup::new("test")
        .override_args(OverrideArgs::new())
        .query_args(QueryArgs::new())
        .arg(Arg::new("output").long("output"))
        .arg(Arg::new("output").long("other")) // Same ID, different long
        .into_bundle(|_cfg: TestConfig| {});
}

// ---------------------------------------------------------------------------------------------- //
// Test 8: Query command returns Handled

#[test]
fn query_command_returns_handled() {
    let bundle = Setup::new("test")
        .override_args(OverrideArgs::new())
        .query_args(QueryArgs::new().desc("desc", None))
        .config_source(|_| {
            ConfigSource::new().positional("CONFIG", "Path to config file", Required::No).loader(
                |_path: &Path| -> Result<Node, std::convert::Infallible> {
                    panic!("loader should not be called for --desc")
                },
            )
        })
        .into_bundle(|_cfg: TestConfig| -> Result<(), io::Error> {
            panic!("handler should not be called for --desc")
        });

    let result = bundle.run_from(["test", "--desc"]);

    // Query command handled the request - returns Ok(Payload::Handled)
    assert!(matches!(result, Ok(None)));
}

// ---------------------------------------------------------------------------------------------- //
// Test 9: Handler result type is preserved

#[test]
fn handler_result_type_preserved() {
    #[derive(Debug, PartialEq)]
    struct CustomOutput {
        message: String,
    }

    let bundle = Setup::new("test")
        .override_args(OverrideArgs::new())
        .query_args(QueryArgs::new())
        .config_source(|c| {
            c.positional("CONFIG", "Path to config file", Required::Yes).loader(
                |_path: &Path| -> Result<Node, std::convert::Infallible> {
                    Ok(Node::parse_str(r#"{"value": "test"}"#, Format::Json).unwrap())
                },
            )
        })
        .into_bundle(|cfg: TestConfig| CustomOutput {
            message: format!("Processed: {}", cfg.value),
        });

    let result = bundle.run_from(["test", "config.json"]);

    match result {
        Ok(Some(output)) => {
            assert_eq!(output.message, "Processed: test");
        }
        other => panic!("Expected Ok(Some(CustomOutput)), got {:?}", other),
    }
}
