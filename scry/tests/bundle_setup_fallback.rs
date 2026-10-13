//! Tests Setup handlers that also provide application subcommands.
#![cfg(feature = "format-json")]

use std::cell::Cell;
use std::convert::Infallible;
use std::path::Path;
use std::rc::Rc;

use scry::clap::{Arg, Command};
use scry::cli::setup::{ConfigSource, Setup, SetupError};
use scry::cli::Bundle;
use scry::node::{Format, NodeError};
use scry::{Config, Desc, Describe, FromNode, Node};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config)]
struct Input {
    /// Selects the input value.
    value: String,
}

struct DecodeTrap;

impl FromNode for DecodeTrap {
    fn from_node(_: &Node) -> Result<Self, NodeError> {
        panic!("terminal requests and child dispatch must not decode the parent input");
    }
}

impl Describe for DecodeTrap {
    fn describe() -> Desc {
        Input::describe()
    }
}

#[test]
fn typed_parent_execution_remains_the_fallback_with_application_subcommands() {
    let loads = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let payload_calls = calls.clone();
    let child = Bundle::single(Command::new("child"), |_| {
        panic!("parent invocation must not dispatch a child");
    });
    let result = parent_setup(loads.clone())
        .expose(|expose| {
            expose.positional("value");
        })
        .into_bundle(move |input: Input| {
            payload_calls.set(payload_calls.get() + 1);
            input.value
        })
        .with_subcommands(vec![child])
        .run_from([
            "parent",
            "-C",
            "input.json",
            "application",
            "--scry",
            "--set",
            "value",
            "edited",
        ])
        .unwrap();

    assert_eq!(result.as_deref(), Some("edited"));
    assert_eq!(loads.get(), 1);
    assert_eq!(calls.get(), 1);
}

#[test]
fn parent_terminal_requests_keep_the_setup_handler_and_skip_the_payload() {
    for suffix in [vec!["get"], vec!["desc"], vec!["--help"], vec![]] {
        let loads = Rc::new(Cell::new(0));
        let child = Bundle::single(Command::new("child"), |_| {
            panic!("parent toolbox must not dispatch a child");
        });
        let mut arguments = vec!["parent", "-C", "input.json", "--scry"];
        arguments.extend_from_slice(&suffix);
        let result = parent_setup(loads.clone())
            .into_bundle(|_: DecodeTrap| -> String {
                panic!("terminal requests must not execute the parent payload");
            })
            .with_subcommands(vec![child])
            .run_from(arguments)
            .unwrap();

        assert!(result.is_none(), "{suffix:?}");
        assert_eq!(loads.get(), usize::from(suffix.first() == Some(&"get")), "{suffix:?}");
    }
}

#[test]
fn child_dispatch_skips_parent_loading_and_preserves_configured_global_exposures() {
    for arguments in [
        ["parent", "--select", "chosen", "child", "item"],
        ["parent", "child", "--select", "chosen", "item"],
    ] {
        let loads = Rc::new(Cell::new(0));
        let child_calls = Rc::new(Cell::new(0));
        let handler_calls = child_calls.clone();
        let child = Bundle::single(Command::new("child").arg(Arg::new("input")), move |matches| {
            handler_calls.set(handler_calls.get() + 1);
            assert_eq!(matches.get_one::<String>("input").map(String::as_str), Some("item"));
            Ok(Some(matches.get_one::<String>("select").cloned().unwrap()))
        });
        let result = parent_setup(loads.clone())
            .expose(|expose| {
                expose.option("value").long("select").global(true);
            })
            .into_bundle(|_: DecodeTrap| -> String {
                panic!("selecting a child must not execute the parent payload");
            })
            .with_subcommands(vec![child])
            .run_from(arguments)
            .unwrap();

        assert_eq!(result.as_deref(), Some("chosen"));
        assert_eq!(loads.get(), 0);
        assert_eq!(child_calls.get(), 1);
    }
}

#[test]
fn nested_toolbox_errors_use_the_parsed_command_path_for_parent_and_children() {
    for (arguments, expected_usage) in [
        (
            vec!["ignored", "group", "parent", "--scry", "unknown"],
            "configured-app group parent --scry",
        ),
        (
            vec!["ignored", "group", "parent", "child", "--scry", "unknown"],
            "configured-app group parent child --scry",
        ),
    ] {
        let loads = Rc::new(Cell::new(0));
        let child = Setup::new("child")
            .into_bundle(|_: DecodeTrap| -> String { panic!("invalid requests must not execute") });
        let parent = parent_setup(loads.clone())
            .into_bundle(|_: DecodeTrap| -> String { panic!("invalid requests must not execute") })
            .with_subcommands(vec![child]);
        let bundle = Bundle::group_with(
            Command::new("application").bin_name("configured-app"),
            vec![Bundle::group("group", "Groups operations.", vec![parent])],
        );
        let error = bundle.run_from(arguments).unwrap_err();

        assert!(matches!(&error, SetupError::Gateway(_)), "{error}");
        assert!(error.to_string().contains(expected_usage), "{error}");
        assert_eq!(loads.get(), 0);
    }
}

#[test]
fn extending_a_group_preserves_existing_children_and_requires_a_subcommand() {
    for name in ["first", "second"] {
        let first = Bundle::single(Command::new("first"), |_| Ok::<_, Infallible>("first"));
        let second = Bundle::single(Command::new("second"), |_| Ok("second"));
        let group = Bundle::group("group", "Groups operations.", vec![first])
            .with_subcommands(vec![second]);

        assert!(group.command().clone().try_get_matches_from(["group"]).is_err());
        assert_eq!(group.run_from(["group", name]).unwrap(), name);
    }
}

#[test]
fn extending_a_group_preserves_its_fallback() {
    for (arguments, expected) in [
        (vec!["group"], "fallback"),
        (vec!["group", "first"], "first"),
        (vec!["group", "second"], "second"),
    ] {
        let first = Bundle::single(Command::new("first"), |_| Ok::<_, Infallible>("first"));
        let second = Bundle::single(Command::new("second"), |_| Ok("second"));
        let group = Bundle::group("group", "Groups operations.", vec![first])
            .fallback(|_| Ok("fallback"))
            .with_subcommands(vec![second]);

        assert_eq!(group.run_from(arguments).unwrap(), expected);
    }
}

#[test]
fn single_handlers_can_consume_captures_and_handle_native_clap_subcommands() {
    let message = String::from("handled");
    let command = Command::new("parent").subcommand(Command::new("native").arg(Arg::new("input")));
    let bundle = Bundle::single(command, move |matches| {
        let (name, child_matches) = matches.subcommand().unwrap();
        assert_eq!(name, "native");
        assert_eq!(child_matches.get_one::<String>("input").map(String::as_str), Some("item"));
        Ok::<_, Infallible>(message)
    });

    assert_eq!(bundle.run_from(["parent", "native", "item"]).unwrap(), "handled");
}

fn parent_setup(loads: Rc<Cell<usize>>) -> Setup {
    Setup::new("parent").config_source(|_| {
        ConfigSource::new().option("config-file", Some('C'), "Loads input.").loader(
            move |_: &Path| -> Result<Node, Infallible> {
                loads.set(loads.get() + 1);
                Ok(Node::parse_str(r#"{ "value": "loaded" }"#, Format::Json).unwrap())
            },
        )
    })
}
