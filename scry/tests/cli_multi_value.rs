//! Integration coverage for CLI arity, value grouping, and append operations.
#![cfg(feature = "format-json")]

use std::convert::Infallible;
use std::path::Path;

use clap::{Arg, Command};
use scry::cli::setup::{
    Arity, ConfigSource, ExposeMap, OverrideArgs, QueryArgs, Required, Setup, SetupError,
    ValueShape,
};
use scry::node::{Format, Value};
use scry::{Config, Describe, Node};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config)]
struct ValuesConfig {
    /// The selected values.
    #[scry(default = vec!["default".to_string()])]
    values: Vec<String>,
    /// An unrelated scalar option.
    output: Option<String>,
}

#[test]
fn named_arrays_convert_vectors_fixed_arrays_and_heterogeneous_tuples() {
    #[derive(Debug, Config)]
    struct Shapes {
        vector: Vec<u32>,
        fixed: [u32; 4],
        mixed: (String, u32, bool),
    }

    let result = Setup::new("test")
        .expose(|e| {
            e.option("vector").array(1..);
            e.option("fixed").array(4);
            e.option("mixed").array(3);
        })
        .into_bundle(|cfg: Shapes| cfg)
        .run_from([
            "test", "--vector", "1", "2", "--fixed", "3", "4", "5", "6", "--mixed", "name", "7",
            "true",
        ])
        .unwrap()
        .unwrap();

    assert_eq!(result.vector, [1, 2]);
    assert_eq!(result.fixed, [3, 4, 5, 6]);
    assert_eq!(result.mixed, ("name".to_string(), 7, true));
}

#[test]
fn array_queries_run_before_typed_conversion_or_the_handler() {
    #[derive(Debug, Config)]
    #[allow(dead_code)]
    struct NumericValues {
        values: Vec<u32>,
    }

    let result = Setup::new("test")
        .query_args(QueryArgs::standard())
        .expose(|e| {
            e.option("values").array(1..);
        })
        .into_bundle(|_: NumericValues| panic!("queries must skip the handler"))
        .run_from([
            "test",
            "--values",
            "001",
            "not-a-number",
            "--get-as",
            "json",
            "values",
        ])
        .unwrap();
    assert!(result.is_none());
}

#[test]
fn array_of_one_remains_an_array_and_preserves_raw_strings() {
    let node = apply("{}", &["test", "--values", "001"], |e| {
        e.option("values").array(1);
    })
    .unwrap();
    let values = node.req_node("values").unwrap().as_vec().unwrap();
    assert_eq!(values.len(), 1);
    assert!(matches!(values[0].read_leaf("string").unwrap(), Value::String(s) if s == "001"));
}

#[test]
fn array_replacement_wraps_the_complete_group_in_a_variant() {
    let node = apply("{}", &["test", "--files", "a", "b"], |e| {
        e.option("values").variant("files").array(2);
    })
    .unwrap();
    assert_eq!(node.req::<Vec<String>>("values.files").unwrap(), ["a", "b"]);
}

#[test]
fn grouped_append_preserves_variable_occurrence_boundaries() {
    let node = apply(
        r#"{ "values": [["loaded"]] }"#,
        &[
            "test", "--values", "a", "b", "--values", "c", "--values", "d", "e", "f",
        ],
        |e| {
            e.option("values").array(1..).append();
        },
    )
    .unwrap();

    assert_eq!(
        node.req::<Vec<Vec<String>>>("values").unwrap(),
        [
            vec!["loaded"],
            vec!["a", "b"],
            vec!["c"],
            vec!["d", "e", "f"]
        ]
    );
}

#[test]
fn grouped_append_of_one_never_flattens() {
    let node = apply("{}", &["test", "--values", "a", "--values", "b"], |e| {
        e.option("values").array(1).append();
    })
    .unwrap();
    assert_eq!(node.req::<Vec<Vec<String>>>("values").unwrap(), [vec!["a"], vec!["b"]]);
}

#[test]
fn short_only_grouped_appends_work_in_either_modifier_order() {
    for append_first in [false, true] {
        let node = apply("{}", &["test", "-v", "a", "b", "-v", "c", "d"], |e| {
            let entry = e.option("values").no_long().short('v');
            if append_first {
                entry.append().array(2);
            } else {
                entry.array(2).append();
            }
        })
        .unwrap();
        assert_eq!(
            node.req::<Vec<Vec<String>>>("values").unwrap(),
            [vec!["a", "b"], vec!["c", "d"]]
        );
    }
}

#[test]
fn flat_append_batches_extend_loaded_values_in_order() {
    let node = apply(r#"{ "values": ["loaded"] }"#, &["test", "-i", "a", "b", "-i", "c"], |e| {
        e.option("values").short('i').append().num_args(1..);
    })
    .unwrap();
    assert_eq!(node.req::<Vec<String>>("values").unwrap(), ["loaded", "a", "b", "c"]);
}

#[test]
fn array_replacement_discards_loaded_values() {
    let node = apply(r#"{ "values": ["loaded"] }"#, &["test", "--values", "a", "b"], |e| {
        e.option("values").array(2);
    })
    .unwrap();
    assert_eq!(node.req::<Vec<String>>("values").unwrap(), ["a", "b"]);
}

#[test]
fn absent_arrays_leave_loaded_values_unchanged() {
    let node = apply(r#"{ "values": ["loaded"] }"#, &["test"], |e| {
        e.option("values").array(2);
    })
    .unwrap();
    assert_eq!(node.req::<Vec<String>>("values").unwrap(), ["loaded"]);
}

#[test]
fn absent_append_uses_defaults_but_present_append_starts_with_cli_values() {
    for (args, expected) in [
        (vec!["test"], vec!["default"]),
        (vec!["test", "--values", "requested"], vec!["requested"]),
    ] {
        let result = Setup::new("test")
            .expose(|e| {
                e.option("values").append();
            })
            .into_bundle(|cfg: ValuesConfig| cfg.values)
            .run_from(args)
            .unwrap()
            .unwrap();
        assert_eq!(result, expected);
    }
}

#[test]
fn append_rejects_existing_scalar_map_and_null_values() {
    for json in [
        r#"{ "values": "scalar" }"#,
        r#"{ "values": { "nested": "value" } }"#,
        r#"{ "values": null }"#,
    ] {
        for grouped in [false, true] {
            let result = apply(json, &["test", "--values", "a"], |e| {
                let entry = e.option("values").append();
                if grouped {
                    entry.array(1);
                }
            });
            assert!(result.is_err(), "append must reject {json}, grouped={grouped}");
        }
    }
}

#[test]
fn arrays_and_flat_appends_interleave_with_set_and_remove() {
    let node = apply(
        r#"{ "values": ["loaded"] }"#,
        &[
            "test",
            "--values",
            "a",
            "b",
            "--set",
            "values[0]",
            "edited",
            "--remove",
            "values[1]",
            "--extra",
            "c",
            "d",
        ],
        |e| {
            e.option("values").array(2);
            e.option("values").long("extra").append().num_args(1..);
        },
    )
    .unwrap();
    assert_eq!(node.req::<Vec<String>>("values").unwrap(), ["edited", "c", "d"]);
}

#[test]
fn grouped_appends_interleave_with_mutations_without_losing_occurrences() {
    let node = apply(
        "{}",
        &[
            "test",
            "--values",
            "a",
            "b",
            "--set",
            "values[0][1]",
            "edited",
            "--values",
            "c",
            "--remove",
            "values[0]",
            "--values",
            "d",
            "e",
        ],
        |e| {
            e.option("values").array(1..).append();
        },
    )
    .unwrap();
    assert_eq!(node.req::<Vec<Vec<String>>>("values").unwrap(), [vec!["c"], vec!["d", "e"]]);
}

#[test]
fn remove_then_append_recreates_an_array() {
    let node = apply(
        r#"{ "values": ["loaded"] }"#,
        &["test", "--remove", "values", "--values", "new"],
        |e| {
            e.option("values").append();
        },
    )
    .unwrap();
    assert_eq!(node.req::<Vec<String>>("values").unwrap(), ["new"]);
}

#[test]
fn scalar_options_keep_their_scalar_shape() {
    let node = apply("{}", &["test", "--output", "a,b c"], |e| {
        e.option("output");
    })
    .unwrap();
    assert!(node.req_node("output").unwrap().kind.is_leaf());
    assert_eq!(node.req::<String>("output").unwrap(), "a,b c");
}

#[test]
fn comma_spaces_and_empty_strings_are_individual_literal_values() {
    let node = apply("{}", &["test", "--values", "a,b", "two words", ""], |e| {
        e.option("values").array(3);
    })
    .unwrap();
    assert_eq!(node.req::<Vec<String>>("values").unwrap(), ["a,b", "two words", ""]);
}

#[test]
fn named_ranges_stop_at_following_options() {
    let node = apply("{}", &["test", "--values", "a", "b", "--output", "result"], |e| {
        e.option("values").array(1..);
        e.option("output");
    })
    .unwrap();
    assert_eq!(node.req::<Vec<String>>("values").unwrap(), ["a", "b"]);
    assert_eq!(node.req::<String>("output").unwrap(), "result");
}

#[test]
fn equals_attachment_closes_the_occurrence_before_a_following_positional() {
    let node = apply("{}", &["test", "--values=a", "b", "--output", "result"], |e| {
        e.option("values").array(1..);
        e.option("output");
        e.positional("tail");
    })
    .unwrap();
    assert_eq!(node.req::<Vec<String>>("values").unwrap(), ["a"]);
    assert_eq!(node.req::<String>("tail").unwrap(), "b");
    assert_eq!(node.req::<String>("output").unwrap(), "result");
}

#[test]
fn equals_attachment_allows_a_hyphenated_value() {
    let node = apply("{}", &["test", "--values=-literal"], |e| {
        e.option("values").array(1);
    })
    .unwrap();
    assert_eq!(node.req::<Vec<String>>("values").unwrap(), ["-literal"]);
}

#[test]
fn named_ranges_reject_unknown_options_and_bare_negative_values() {
    for args in [
        vec!["test", "--values", "a", "--unknown"],
        vec!["test", "--values", "-5"],
        vec!["test", "--values", "a", "--", "b"],
    ] {
        assert!(
            parse(&args, |e| {
                e.option("values").array(1..);
            })
            .is_err(),
            "expected a parsing error for {args:?}"
        );
    }
}

#[test]
fn fixed_and_ranged_arities_enforce_minimum_and_maximum() {
    for args in [
        vec!["test", "--values"],
        vec!["test", "--values", "a"],
        vec!["test", "--values", "a", "b", "c"],
    ] {
        assert!(parse(&args, |e| {
            e.option("values").array(2);
        })
        .is_err());
    }
    for count in 0..=4 {
        let mut args = vec!["test", "--values"];
        args.extend(std::iter::repeat_n("entry", count));
        let result = parse(&args, |e| {
            e.option("values").array(1..=3);
        });
        assert_eq!(result.is_ok(), (1..=3).contains(&count), "{args:?}");
    }
}

#[test]
fn replacement_options_reject_repeated_occurrences() {
    for grouped in [false, true] {
        let error = parse(&["test", "--values", "a", "--values", "b"], |e| {
            let entry = e.option("values");
            if grouped {
                entry.array(1);
            }
        })
        .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
}

#[test]
fn clap_defaults_do_not_become_config_operations() {
    for grouped in [false, true] {
        let mut expose = ExposeMap::new();
        let entry = expose.option("values").append();
        if grouped {
            entry.array(1);
        }
        let command = expose
            .augment(Command::new("test"), &ValuesConfig::describe())
            .mut_arg("values", |arg| arg.default_value("clap-default"));
        let matches = command.try_get_matches_from(["test"]).unwrap();
        let mut node = Node::parse_str(r#"{ "values": ["loaded"] }"#, Format::Json).unwrap();
        OverrideArgs::new().apply::<ValuesConfig>(&mut node, &expose, &matches).unwrap();
        assert_eq!(node.req::<Vec<String>>("values").unwrap(), ["loaded"]);
    }
}

#[test]
fn malformed_scalar_matches_return_an_error_before_applying_overrides() {
    let mut expose = ExposeMap::new();
    expose.option("output");
    let command = expose
        .augment(Command::new("test"), &ValuesConfig::describe())
        .mut_arg("output", |arg| arg.num_args(2));
    let matches = command.try_get_matches_from(["test", "--output", "one", "two"]).unwrap();
    let mut node = Node::parse_str(r#"{ "output": "loaded" }"#, Format::Json).unwrap();
    let result = OverrideArgs::new().apply::<ValuesConfig>(&mut node, &expose, &matches);
    assert!(result.is_err());
    assert_eq!(node.req::<String>("output").unwrap(), "loaded");
}

#[test]
fn fixed_assignments_reject_missing_argument_ids_and_wrong_match_types() {
    let mut expose = ExposeMap::new();
    expose.flag("output", "assigned");
    let matches = [
        Command::new("test").try_get_matches_from(["test"]).unwrap(),
        Command::new("test")
            .arg(Arg::new("output").long("output"))
            .try_get_matches_from(["test", "--output", "wrong-type"])
            .unwrap(),
    ];
    for matches in matches {
        let mut node = Node::parse_str(r#"{ "output": "loaded" }"#, Format::Json).unwrap();
        let error =
            OverrideArgs::new().apply::<ValuesConfig>(&mut node, &expose, &matches).unwrap_err();
        assert!(matches!(error, SetupError::InvalidCliMatches { .. }));
        assert_eq!(node.req::<String>("output").unwrap(), "loaded");
    }
}

#[test]
fn public_override_application_rejects_missing_override_argument_ids() {
    let matches = Command::new("test").try_get_matches_from(["test"]).unwrap();
    let mut node = Node::empty_map();
    let error = OverrideArgs::standard()
        .apply::<ValuesConfig>(&mut node, &ExposeMap::new(), &matches)
        .unwrap_err();
    assert!(matches!(error, SetupError::InvalidCliMatches { .. }));
}

#[test]
fn positional_array_collects_runs_and_assigns_at_its_final_value() {
    let node = apply(
        r#"{ "values": ["loaded"] }"#,
        &[
            "test",
            "first",
            "--set",
            "values[0]",
            "intermediate",
            "second",
            "--set",
            "values[1]",
            "final",
        ],
        |e| {
            e.positional("values").array(1..);
        },
    )
    .unwrap();
    assert_eq!(node.req::<Vec<String>>("values").unwrap(), ["first", "final"]);
}

#[test]
fn absent_positional_array_preserves_loaded_values_and_defaults() {
    let node = apply(r#"{ "values": ["loaded"] }"#, &["test"], |e| {
        e.positional("values").array(1..);
    })
    .unwrap();
    assert_eq!(node.req::<Vec<String>>("values").unwrap(), ["loaded"]);

    let values = Setup::new("test")
        .expose(|e| {
            e.positional("values").array(1..);
        })
        .into_bundle(|cfg: ValuesConfig| cfg.values)
        .run_from(["test"])
        .unwrap()
        .unwrap();
    assert_eq!(values, ["default"]);
}

#[test]
fn end_of_options_routes_hyphenated_values_to_positional_array() {
    let result = Setup::new("test")
        .expose(|e| {
            e.positional("values").array(1..);
            e.option("output");
        })
        .into_bundle(|cfg: ValuesConfig| cfg)
        .run_from([
            "test", "--output", "result", "--", "-5", "--output", "literal",
        ])
        .unwrap()
        .unwrap();
    assert_eq!(result.values, ["-5", "--output", "literal"]);
    assert_eq!(result.output.as_deref(), Some("result"));
}

#[test]
fn positional_array_supports_named_config_and_a_preceding_required_scalar() {
    let result = Setup::new("test")
        .config_source(|_| {
            ConfigSource::new().option("config", Some('C'), "Loads the config.").loader(
                |_: &Path| -> Result<Node, Infallible> {
                    Ok(Node::parse_str(r#"{ "values": ["loaded"] }"#, Format::Json).unwrap())
                },
            )
        })
        .arg(Arg::new("command-input").index(1).required(true))
        .expose(|e| {
            e.positional("values").array(1..);
        })
        .into_bundle_with_matches(|cfg: ValuesConfig, matches| {
            (cfg.values, matches.get_one::<String>("command-input").unwrap().clone())
        })
        .run_from(["test", "-C", "config.json", "input", "a", "b"])
        .unwrap()
        .unwrap();
    assert_eq!(result, (vec!["a".to_string(), "b".to_string()], "input".to_string()));
}

#[test]
fn value_names_show_named_components_and_range_placeholders() {
    let mut expose = ExposeMap::new();
    expose.option("values").array(2).value_names(["LEFT", "RIGHT"]);
    expose.option("output").append().num_args(1..).value_name("DEST");
    let help =
        expose.augment(Command::new("test"), &ValuesConfig::describe()).render_help().to_string();
    assert!(help.contains("--values <LEFT> <RIGHT>"), "{help}");
    assert!(help.contains("--output <DEST>..."), "{help}");
}

#[test]
fn invalid_value_shapes_are_rejected_at_construction() {
    let cases: &[fn(&mut ExposeMap)] = &[
        |e| {
            e.option("values").array(0);
        },
        |e| {
            e.option("values").array(0..);
        },
        |e| {
            e.option("values").append().num_args(0..=2);
        },
        |e| {
            e.option("values").num_args(2);
        },
        |e| {
            e.option("values").num_args(1..);
        },
        |e| {
            e.positional("values").append();
        },
        |e| {
            e.positional("values").array(2);
        },
        |e| {
            e.positional("values").array(1..=3);
        },
        |e| {
            e.option("values").variant("files").append();
        },
        |e| {
            e.option("values").append().variant("files");
        },
        |e| {
            e.flag("values", true).array(2);
        },
        |e| {
            e.flag("values", true).append();
        },
        |e| {
            e.flag("values", true).num_args(1);
        },
        |e| {
            e.option("values").array(2).value_names(["ONE"]);
        },
        |e| {
            e.option("values").array(1..).value_names(["ONE", "TWO"]);
        },
    ];
    for (index, configure) in cases.iter().enumerate() {
        let result = std::panic::catch_unwind(|| {
            let mut expose = ExposeMap::new();
            configure(&mut expose);
            expose.augment(Command::new("test"), &ValuesConfig::describe()).debug_assert();
        });
        assert!(result.is_err(), "invalid configuration {index} was accepted");
    }
}

#[test]
fn named_arguments_cannot_silently_become_positionals() {
    let cases: &[fn(&mut ExposeMap)] = &[
        |e| {
            e.option("values").no_long();
        },
        |e| {
            e.option("values").array(1..).no_long();
        },
        |e| {
            e.option("values").append().no_long();
        },
        |e| {
            e.flag("output", "assigned").no_long();
        },
    ];
    for (index, configure) in cases.iter().enumerate() {
        let result = std::panic::catch_unwind(|| {
            let mut expose = ExposeMap::new();
            configure(&mut expose);
            expose.augment(Command::new("test"), &ValuesConfig::describe()).debug_assert();
        });
        assert!(result.is_err(), "nameless named argument {index} was accepted");
    }
}

#[test]
fn direct_metadata_cannot_bypass_validation() {
    let cases: &[fn(&mut ExposeMap)] = &[
        |e| {
            e.option("values").arity = Arity { min: 0, max: None };
        },
        |e| {
            e.option("values").array(2).arity = Arity {
                min: 3,
                max: Some(2),
            };
        },
        |e| {
            e.option("values").arity = Arity {
                min: 2,
                max: Some(2),
            };
        },
        |e| {
            e.positional("values").append = true;
        },
        |e| {
            e.option("values").variant("files").append = true;
        },
        |e| {
            e.flag("values", true).shape = ValueShape::Array;
        },
        |e| {
            let entry = e.option("values").array(2);
            entry.value_name = Some("VALUE".to_string());
            entry.value_names = Some(vec!["LEFT".to_string(), "RIGHT".to_string()]);
        },
    ];
    for (index, configure) in cases.iter().enumerate() {
        let result = std::panic::catch_unwind(|| {
            let mut expose = ExposeMap::new();
            configure(&mut expose);
            expose.augment(Command::new("test"), &ValuesConfig::describe()).debug_assert();
        });
        assert!(result.is_err(), "invalid direct metadata configuration {index} was accepted");
    }
}

#[test]
fn positional_arrays_reject_ambiguous_layouts() {
    let cases: &[fn() -> Setup] = &[
        || {
            Setup::new("test").expose(|e| {
                e.positional("output");
                e.positional("values").array(1..);
            })
        },
        || {
            Setup::new("test").expose(|e| {
                e.positional("values").array(1..);
                e.positional("output");
            })
        },
        || {
            Setup::new("test").expose(|e| {
                e.positional("values").array(1..);
                e.positional("output").array(1..);
            })
        },
        || {
            Setup::new("test")
                .config_source(|c| c.positional("CONFIG", "Loads the config.", Required::Yes))
                .expose(|e| {
                    e.positional("values").array(1..);
                })
        },
        || {
            Setup::new("test")
                .config_source(|c| c.positional("CONFIG", "Loads the config.", Required::No))
                .expose(|e| {
                    e.positional("values").array(1..);
                })
        },
        || {
            Setup::new("test").arg(Arg::new("other").index(1)).expose(|e| {
                e.positional("values").array(1..);
            })
        },
        || {
            Setup::new("test")
                .arg(Arg::new("other").index(1).required(true).value_names(["LEFT", "RIGHT"]))
                .expose(|e| {
                    e.positional("values").array(1..);
                })
        },
        || {
            Setup::new("test")
                .arg(Arg::new("other").index(1).required(true).action(clap::ArgAction::SetTrue))
                .expose(|e| {
                    e.positional("values").array(1..);
                })
        },
        || {
            Setup::new("test")
                .arg(Arg::new("other").index(1).required(true).num_args(1).value_delimiter(','))
                .expose(|e| {
                    e.positional("values").array(1..);
                })
        },
    ];
    for (index, setup) in cases.iter().enumerate() {
        let result = std::panic::catch_unwind(|| {
            setup().into_bundle(|cfg: ValuesConfig| cfg).command().clone().debug_assert();
        });
        assert!(result.is_err(), "ambiguous positional layout {index} was accepted");
    }
}

// ---------------------------------------------------------------------------------------------- //

fn parse(
    args: &[&str],
    configure: impl FnOnce(&mut ExposeMap),
) -> Result<clap::ArgMatches, clap::Error> {
    let mut expose = ExposeMap::new();
    configure(&mut expose);
    expose.augment(Command::new("test"), &ValuesConfig::describe()).try_get_matches_from(args)
}

fn apply(
    json: &str,
    args: &[&str],
    configure: impl FnOnce(&mut ExposeMap),
) -> Result<Node, Box<dyn std::error::Error>> {
    let mut expose = ExposeMap::new();
    configure(&mut expose);
    let overrides = OverrideArgs::standard();
    let command =
        overrides.augment(expose.augment(Command::new("test"), &ValuesConfig::describe()));
    let matches = command.try_get_matches_from(args)?;
    let mut node = Node::parse_str(json, Format::Json)?;
    overrides.apply::<ValuesConfig>(&mut node, &expose, &matches)?;
    Ok(node)
}
