//! Integration tests for terse default-override annotations in `--get` tree output.
//!
//! `--get` renders the user-supplied input tree; tree output appends ` [<default>]` to leaves
//! whose value overrides a different default. Omitted defaults and equal-to-default values are
//! left untouched.
#![cfg(feature = "format-json")]

use scry::cli::setup::{
    format_raw_at_path, format_tree_at_path, render_with_default_annotations, GetFormat,
};
use scry::node::Format;
use scry::writer::TreeConfig;
use scry::{Config, DefaultNode, Describe, FromNode, Node, ToNode};

// ---------------------------------------------------------------------------------------------- //
// Test Config Types

/// Server deployment configuration.
#[derive(Debug, Config)]
#[allow(dead_code)]
struct Deploy {
    /// Target server hostname (required, no default).
    target: String,
    /// Number of retry attempts.
    #[scry(default = 3)]
    retries: u32,
    /// Optional timeout in seconds.
    timeout_secs: Option<u64>,
    /// Run without making changes.
    #[scry(default = false)]
    dry_run: bool,
    /// Row/column ordering, defaulting to a string value.
    #[scry(default = String::from("row_major"))]
    process_order: String,
    /// Optional notification settings.
    notify: Option<Notify>,
}

/// Notification settings.
#[derive(Debug, Config)]
#[allow(dead_code)]
struct Notify {
    /// Slack webhook URL (required, no default).
    slack_url: String,
    /// Only notify on failure.
    #[scry(default = false)]
    on_failure_only: bool,
}

/// A type whose every field is required, so it has no default baseline.
#[derive(Debug, Config)]
#[allow(dead_code)]
struct AllRequired {
    a: String,
    b: u32,
}

// ---------------------------------------------------------------------------------------------- //
// Helpers

fn json(s: &str) -> Node {
    Node::parse_str(s, Format::Json).expect("valid JSON test input")
}

/// Renders the whole input tree with terse default annotations, color disabled.
fn render<T>(input_json: &str) -> String
where
    T: FromNode + Describe + ToNode + DefaultNode,
{
    let input = json(input_json);
    let config: T = input.as_type().expect("test input should parse");
    render_with_default_annotations(&input, &config, &TreeConfig::no_color())
        .expect("render should succeed")
}

// ---------------------------------------------------------------------------------------------- //
// Annotation Behavior

#[test]
fn changed_scalar_default_shows_terse_suffix() {
    let out = render::<Deploy>(r#"{ "target": "host", "retries": 5 }"#);
    assert!(out.contains("retries 5 [3]"), "got:\n{out}");
    // The required, no-default field gets no suffix.
    assert!(out.contains(r#"target "host""#) && !out.contains("target \"host\" ["), "got:\n{out}");
}

#[test]
fn equal_to_default_shows_no_suffix() {
    let out = render::<Deploy>(r#"{ "target": "host", "retries": 3 }"#);
    assert!(out.contains("retries 3"), "got:\n{out}");
    assert!(!out.contains('['), "equal-to-default must not be annotated; got:\n{out}");
}

#[test]
fn string_default_renders_quoted_suffix() {
    let out = render::<Deploy>(r#"{ "target": "host", "process_order": "col_major" }"#);
    assert!(out.contains(r#"process_order "col_major" ["row_major"]"#), "got:\n{out}");
}

#[test]
fn nested_supplied_field_can_be_annotated() {
    let out = render::<Deploy>(
        r#"{ "target": "host", "notify": { "slack_url": "u", "on_failure_only": true } }"#,
    );
    assert!(out.contains("on_failure_only true [false]"), "got:\n{out}");
    // The required nested field is shown without a suffix.
    assert!(out.contains(r#"slack_url "u""#), "got:\n{out}");
}

#[test]
fn omitted_defaults_are_not_rendered() {
    // Only the required field is supplied; everything else is defaulted or optional-absent.
    let out = render::<Deploy>(r#"{ "target": "host" }"#);
    assert_eq!(out, r#"▸ target "host""#, "omitted defaults must not appear; got:\n{out}");
}

// ---------------------------------------------------------------------------------------------- //
// Default Baseline Generation

#[test]
fn default_node_contains_only_defaulted_paths() {
    let baseline = Deploy::default_node().expect("ok").expect("Deploy has defaults");

    assert_eq!(baseline.opt_node("retries").unwrap().unwrap().as_type::<u32>().unwrap(), 3);
    assert!(!baseline.opt_node("dry_run").unwrap().unwrap().as_type::<bool>().unwrap());
    assert_eq!(
        baseline.opt_node("process_order").unwrap().unwrap().as_type::<String>().unwrap(),
        "row_major"
    );

    // Required and optional scalar fields without a default carry no baseline.
    assert!(baseline.opt_node("target").unwrap().is_none());
    assert!(baseline.opt_node("timeout_secs").unwrap().is_none());

    // `notify` is `Option<Notify>` with no field default, but the inner type contributes its
    // own defaults structurally, so a supplied nested block can still be annotated. The
    // baseline is partial: it holds `on_failure_only` but not the required `slack_url`.
    let notify = baseline.opt_node("notify").unwrap().expect("notify carries inner defaults");
    assert!(!notify.opt_node("on_failure_only").unwrap().unwrap().as_type::<bool>().unwrap());
    assert!(notify.opt_node("slack_url").unwrap().is_none());
}

#[test]
fn nested_default_node_is_partial() {
    let baseline = Notify::default_node().expect("ok").expect("Notify has a default");
    assert!(!baseline.opt_node("on_failure_only").unwrap().unwrap().as_type::<bool>().unwrap());
    assert!(baseline.opt_node("slack_url").unwrap().is_none());
}

#[test]
fn all_required_type_has_no_baseline() {
    assert!(AllRequired::default_node().unwrap().is_none());
}

// ---------------------------------------------------------------------------------------------- //
// Flat / Format Output and Path Selection

#[test]
fn flat_output_has_no_annotations_or_omitted_defaults() {
    let input = json(r#"{ "target": "host", "retries": 5 }"#);
    let flat = format_raw_at_path::<Deploy>(&input, "", GetFormat::Flat).unwrap();

    assert!(!flat.contains("[3]"), "flat output must not be annotated; got:\n{flat}");
    assert!(
        !flat.contains("dry_run"),
        "flat output must not invent omitted defaults; got:\n{flat}"
    );
}

#[test]
fn get_omitted_default_path_is_not_found() {
    let input = json(r#"{ "target": "host" }"#);
    let config: Deploy = input.as_type().unwrap();
    // `dry_run` is a valid schema field but absent from the input tree.
    let result = format_tree_at_path::<Deploy>(&input, &config, "dry_run");
    assert!(result.is_err(), "omitted default path should report not-found");
}
