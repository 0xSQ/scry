//! Covers non-exiting bundle execution and executable parser behavior.

use std::cell::Cell;
use std::convert::Infallible;
use std::error::Error;
use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::Command as ProcessCommand;
use std::rc::Rc;

use clap::builder::OsStringValueParser;
use clap::error::ErrorKind;
use clap::{Arg, Command};
use scry::cli::setup::{Setup, SetupError};
use scry::cli::{Bundle, RunError};
use scry::{Config, Node};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config)]
struct Input {
    /// Supplies a numeric value.
    value: u16,
}

#[derive(Debug, PartialEq, Eq)]
struct HandlerFailure;

#[derive(Debug, thiserror::Error)]
#[error("handler failed")]
struct SourcedFailure {
    #[source]
    source: io::Error,
}

#[test]
fn supplied_arguments_dispatch_once_and_consume_the_selected_handler() {
    let captured = "captured output".to_owned();
    let calls = Rc::new(Cell::new(0));
    let handler_calls = calls.clone();
    let result = Bundle::single(
        Command::new("render").arg(Arg::new("value").long("value")),
        move |matches| {
            handler_calls.set(handler_calls.get() + 1);
            Ok::<_, Infallible>((captured, matches.get_one::<String>("value").cloned()))
        },
    )
    .try_run_from(["render", "--value", "selected"])
    .unwrap();

    assert_eq!(result, ("captured output".to_owned(), Some("selected".to_owned())));
    assert_eq!(calls.get(), 1);
}

#[test]
fn parser_outcomes_retain_clap_details_without_dispatching() {
    let calls = Rc::new(Cell::new(0));
    for (arguments, kind, status, stderr, content) in [
        (vec!["atlas", "--unknown"], ErrorKind::UnknownArgument, 2, true, "--unknown"),
        (vec!["atlas", "image"], ErrorKind::MissingSubcommand, 2, true, "atlas image"),
        (vec!["atlas", "--help"], ErrorKind::DisplayHelp, 0, false, "Usage: atlas"),
        (vec!["atlas", "--version"], ErrorKind::DisplayVersion, 0, false, "atlas 1.2.3"),
    ] {
        let error = parsing_bundle(calls.clone()).try_run_from(arguments).unwrap_err();
        let error = match error {
            RunError::Parse(error) => error,
            RunError::Handler(never) => match never {},
        };
        assert_eq!(error.kind(), kind);
        assert_eq!(error.exit_code(), status);
        assert_eq!(error.use_stderr(), stderr);
        assert!(error.to_string().contains(content), "{error}");
    }
    assert_eq!(calls.get(), 0);
}

fn parsing_bundle(calls: Rc<Cell<usize>>) -> Bundle<(), Infallible> {
    let render = Bundle::single(Command::new("render"), move |_| {
        calls.set(calls.get() + 1);
        Ok(())
    });
    Bundle::group_with(
        Command::new("atlas").version("1.2.3"),
        vec![Bundle::group("image", "Processes images.", vec![render])],
    )
}

#[test]
fn handler_errors_need_no_error_or_parser_conversion_trait() {
    let error = Bundle::single(Command::new("render"), |_| Err::<(), _>(HandlerFailure))
        .try_run_from(["render"])
        .unwrap_err();
    let RunError::Handler(error) = error else {
        panic!("expected the original handler error");
    };
    assert_eq!(error, HandlerFailure);
}

#[test]
fn error_reporting_delegates_transparently_and_variants_keep_original_errors() {
    let error = Bundle::single(Command::new("render"), |_| {
        Err::<(), _>(SourcedFailure {
            source: io::Error::new(io::ErrorKind::InvalidData, "original failure"),
        })
    })
    .try_run_from(["render"])
    .unwrap_err();
    let RunError::Handler(original) = &error else {
        panic!("expected the original handler error");
    };
    assert_eq!(original.source.kind(), io::ErrorKind::InvalidData);
    assert_eq!(error.to_string(), "handler failed");
    assert_eq!(
        error.source().unwrap().downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(format!("{:#}", anyhow::Error::new(error)), "handler failed: original failure");

    let error =
        Bundle::single(Command::new("render"), |_| Err::<(), _>("plain failure".to_owned()))
            .try_run_from(["render"])
            .unwrap_err();
    assert_eq!(error.to_string(), "plain failure");
}

#[test]
fn setup_failures_and_payload_results_keep_their_existing_boundaries() {
    let success = typed_bundle().try_run_from(["render", "--value", "7"]).unwrap();
    assert_eq!(success, Some(Ok(7)));

    let error = typed_bundle().try_run_from(["render", "--value", "invalid"]).unwrap_err();
    assert!(matches!(error, RunError::Handler(SetupError::EvaluateInputConfig { .. })));

    let payload = typed_bundle().try_run_from(["render", "--value", "9"]).unwrap();
    assert_eq!(payload, Some(Err(HandlerFailure)));
}

fn typed_bundle() -> Bundle<Option<Result<u16, HandlerFailure>>, SetupError> {
    Setup::new("render")
        .expose(|fields| {
            fields.option("value");
        })
        .into_bundle(|input: Input| {
            if input.value == 9 {
                Err(HandlerFailure)
            } else {
                Ok(input.value)
            }
        })
}

#[test]
fn support_outcomes_remain_handler_owned_and_skip_loading() {
    let error = unloaded_setup()
        .into_bundle(|_: Input| panic!("support parsing must not execute"))
        .try_run_from(["render", "-C", "unused.rhai", "-S", "--unknown"])
        .unwrap_err();
    let RunError::Handler(SetupError::Support(error)) = error else {
        panic!("expected the support parser's setup error");
    };
    assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    assert_eq!(error.exit_code(), 2);
    assert!(error.use_stderr());

    let handled = unloaded_setup()
        .into_bundle(|_: Input| panic!("support help must not execute"))
        .try_run_from(["render", "-C", "unused.rhai", "-S", "--help"])
        .unwrap();
    assert!(handled.is_none());
}

fn unloaded_setup() -> Setup {
    Setup::standard("render").config_source(|source| {
        source.loader(|_: &Path| -> Result<Node, Infallible> {
            panic!("support requests must not load input");
        })
    })
}

#[test]
fn process_argument_runner_passes_native_arguments_to_the_handler() {
    let command = Command::new("arguments").disable_help_flag(true).arg(
        Arg::new("arguments")
            .num_args(0..)
            .allow_hyphen_values(true)
            .trailing_var_arg(true)
            .value_parser(OsStringValueParser::new()),
    );
    let arguments = Bundle::single(command, |matches| {
        Ok::<_, Infallible>(
            matches
                .get_many::<OsString>("arguments")
                .into_iter()
                .flatten()
                .cloned()
                .collect::<Vec<_>>(),
        )
    })
    .try_run()
    .unwrap();
    assert_eq!(arguments, std::env::args_os().skip(1).collect::<Vec<_>>());
}

#[test]
fn legacy_runner_preserves_parser_output_streams_and_exit_statuses() {
    for (argument, status, stdout_text, stderr_text) in [
        ("--help", 0, "Usage: legacy-tool", ""),
        ("--version", 0, "legacy-tool 1.2.3", ""),
        ("--unknown", 2, "", "--unknown"),
    ] {
        let output = ProcessCommand::new(std::env::current_exe().unwrap())
            .args(["--exact", "legacy_runner_probe", "--nocapture"])
            .env("SCRY_TEST_BUNDLE_EXIT_PROBE", argument)
            .output()
            .unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(output.status.code(), Some(status), "{argument}: {stdout}\n{stderr}");
        if stderr_text.is_empty() {
            assert!(stderr.is_empty(), "{argument}: {stderr}");
            assert!(stdout.contains(stdout_text), "{argument}: {stdout}");
        } else {
            assert!(stderr.contains(stderr_text), "{argument}: {stderr}");
            assert!(stderr.contains("Usage: legacy-tool"), "{argument}: {stderr}");
            assert!(!stdout.contains(stderr_text), "{argument}: {stdout}");
        }
    }
}

#[test]
fn legacy_runner_probe() {
    let Ok(argument) = std::env::var("SCRY_TEST_BUNDLE_EXIT_PROBE") else {
        return;
    };
    Bundle::single(Command::new("legacy-tool").version("1.2.3"), |_| -> Result<(), Infallible> {
        panic!("parser requests must not call the handler");
    })
    .run_from(["legacy-tool", argument.as_str()])
    .unwrap();
    panic!("the executable parser policy must exit");
}
