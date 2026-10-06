use super::*;
use crate::node::{Node, Value};

// ---------------------------------------------------------------------------------------------- //

#[test]
fn sourced_value_errors_accept_boxed_causes_and_preserve_them_after_prefixing() {
    let cause: BoxedError = Box::new(std::io::Error::other("domain parser failed"));
    let error =
        NodeError::invalid_value_with_source(&path("detail"), "invalid custom value", cause)
            .prepend_path(&KeyPath::from_index(2))
            .prepend_path(&KeyPath::from_keys(["jobs"]));

    assert_eq!(error.path(), Some(&path("jobs[2].detail")));
    assert_eq!(error.to_string(), "invalid value for 'jobs[2].detail': invalid custom value");
    assert!(error.source().unwrap().is::<std::io::Error>());
    assert_eq!(error.source().unwrap().to_string(), "domain parser failed");
}

#[test]
fn attaching_input_locations_preserves_existing_paths_and_unknown_key_aggregates() {
    let parent = path("parent");
    let error = NodeError::invalid_value(&path("parent.child"), "invalid").at_path(&parent);
    assert_eq!(error.path(), Some(&path("parent.child")));

    let error = NodeError::invalid_value(&KeyPath::new(), "root error").at_path(&parent);
    assert_eq!(error.path(), Some(&KeyPath::new()));

    let paths = [path("parent.first"), path("parent.second")];
    let error = NodeError::unknown_keys(&paths).at_path(&parent);
    assert!(matches!(error, NodeError::UnknownKeys { paths: actual } if actual == paths));
}

#[test]
fn output_prefixes_compose_unknown_key_locations_in_order() {
    let error = NodeError::unknown_keys(&[path("first"), path("second.nested")])
        .prepend_path(&KeyPath::from_index(1))
        .prepend_path(&KeyPath::from_keys(["items"]));

    assert!(matches!(error, NodeError::UnknownKeys { paths }
        if paths == [path("items[1].first"), path("items[1].second.nested")]));
}

#[test]
fn logical_locations_wrap_filesystem_errors_without_changing_the_file_path() {
    let file_path = Path::new("settings.rhai");
    let error = NodeError::read_file(file_path, std::io::Error::other("read failed"))
        .at_path(&path("template"))
        .prepend_path(&path("jobs"));

    assert_eq!(error.path(), Some(&path("jobs.template")));
    assert_eq!(error.to_string(), "error for 'jobs.template': failed to read file: settings.rhai");
    let original = error.source().unwrap().downcast_ref::<NodeError>().unwrap();
    assert!(matches!(original, NodeError::ReadFile { path, .. } if path == file_path));
    assert!(original.path().is_none());
    assert!(original.source().unwrap().is::<std::io::Error>());
}

#[test]
fn repeated_location_attachment_keeps_one_wrapper_and_empty_prefix_is_identity() {
    let error = NodeError::new("failure")
        .at_path(&path("child"))
        .at_path(&path("parent"))
        .prepend_path(&path("parent"))
        .prepend_path(&KeyPath::new());
    assert_eq!(error.path(), Some(&path("parent.child")));
    let original = error.source().unwrap().downcast_ref::<NodeError>().unwrap();
    assert!(matches!(original, NodeError::Message { .. }));
    assert!(original.source().is_none());

    let error = NodeError::new("failure").prepend_path(&KeyPath::new());
    assert!(matches!(error, NodeError::Message { .. }));
}

// ------------------------------------------------------------------------------------------ //
// Display - with path

#[test]
fn display_message() {
    let err = NodeError::new("something went wrong");
    assert_eq!(err.to_string(), "something went wrong");
}

#[test]
fn display_invalid_value() {
    let err = NodeError::invalid_value(&path("server.port"), "must be between 1 and 65535");
    assert_eq!(err.to_string(), "invalid value for 'server.port': must be between 1 and 65535");
}

#[test]
fn display_missing_required() {
    let err = NodeError::missing_required(&path("database.host"));
    assert_eq!(err.to_string(), "missing value for 'database.host'");
}

#[test]
fn display_type_mismatch() {
    let err = NodeError::type_mismatch(&path("server.port"), "string", "i64");
    assert_eq!(err.to_string(), "expected string for 'server.port', found type i64");
}

#[test]
fn display_kind_mismatch() {
    let map_node = Node::new_map(KeyPath::new(), indexmap::IndexMap::new());
    let err = NodeError::kind_mismatch(&path("server"), "array", &map_node.kind);
    assert_eq!(err.to_string(), "expected array for 'server', found type map");
}

#[test]
fn display_invalid_conversion() {
    let err = NodeError::invalid_conversion(&path("timeout"), "u16", "i64", "-5");
    assert_eq!(err.to_string(), "cannot convert 'timeout' to u16 (from i64 '-5')");
}

#[test]
fn display_array_length() {
    let err = NodeError::array_length(&path("color"), 3, 2);
    assert_eq!(err.to_string(), "expected 'color' to be an array of length 3, found 2");
}

#[test]
fn display_key_on_array() {
    let err = NodeError::key_on_array(&path("items"), "name");
    assert_eq!(err.to_string(), "'items' is an array, cannot look up key 'name'");
}

#[test]
fn display_index_on_map() {
    let err = NodeError::index_on_map(&path("server"), 0);
    assert_eq!(err.to_string(), "'server' is a map, cannot look up index 0");
}

#[test]
fn display_index_out_of_bounds() {
    let err = NodeError::index_out_of_bounds(&path("items"), 5, 3);
    assert_eq!(err.to_string(), "index 5 is out of bounds, 'items' has 3 elements");
}

#[test]
fn display_descend_into_leaf() {
    let err = NodeError::descend_into_leaf(&path("server.port"), "i64");
    assert_eq!(err.to_string(), "'server.port' has type i64, cannot descend into it");
}

#[test]
fn display_cannot_remove_root() {
    let err = NodeError::cannot_remove_root();
    assert_eq!(err.to_string(), "cannot remove root node");
}

#[test]
fn display_unknown_keys() {
    let err = NodeError::unknown_keys(&[path("server.foo"), path("server.bar")]);
    assert_eq!(err.to_string(), "unknown config keys:\n  server.foo\n  server.bar");
}

#[test]
fn display_missing_file_extension() {
    let err = NodeError::from(FormatError::missing_file_extension(Path::new("config")));
    assert_eq!(err.to_string(), "cannot determine config format: file has no extension: config");
}

#[test]
fn display_unknown_file_extension() {
    let err = NodeError::from(FormatError::unknown_file_extension(
        Path::new("config.yml"),
        "yml",
        &["json", "json5", "rhai"],
    ));
    assert_eq!(
        err.to_string(),
        "unknown file extension '.yml' for file: config.yml (supported: json, json5, rhai)"
    );
}

#[test]
fn display_unknown_output_format_id() {
    let err = NodeError::from(FormatError::unknown_format_id(
        crate::node::FormatUsage::Output,
        "yaml",
        &["json", "rhai"],
    ));
    assert_eq!(err.to_string(), "unknown output format 'yaml' (supported: json, rhai)");
}

#[test]
fn display_invalid_path() {
    let source = "server..port".parse::<KeyPath>().unwrap_err();
    let err = NodeError::invalid_path(source);
    assert_eq!(err.to_string(), "invalid path");
}

#[test]
fn display_read_file() {
    let source = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
    let err = NodeError::read_file(Path::new("config.toml"), source);
    assert_eq!(err.to_string(), "failed to read file: config.toml");
}

#[test]
fn display_parse_format() {
    let source = std::io::Error::new(std::io::ErrorKind::InvalidData, "bad");
    let err = NodeError::parse_format("JSON", source);
    assert_eq!(err.to_string(), "failed to parse JSON");
}

#[test]
fn display_serialize_format() {
    let source = std::io::Error::other("boom");
    let err = NodeError::serialize_format("Rhai", source);
    assert_eq!(err.to_string(), "failed to serialize as Rhai");
}

// ------------------------------------------------------------------------------------------ //
// Display - empty path (root)

#[test]
fn display_invalid_value_at_root() {
    let err = NodeError::invalid_value(&KeyPath::new(), "expected a map");
    assert_eq!(err.to_string(), "invalid value for config: expected a map");
}

#[test]
fn display_missing_required_at_root() {
    let err = NodeError::missing_required(&KeyPath::new());
    assert_eq!(err.to_string(), "missing value for config");
}

#[test]
fn display_type_mismatch_at_root() {
    let err = NodeError::type_mismatch(&KeyPath::new(), "map", "array");
    assert_eq!(err.to_string(), "expected map for config, found type array");
}

#[test]
fn display_array_length_at_root() {
    let err = NodeError::array_length(&KeyPath::new(), 3, 2);
    assert_eq!(err.to_string(), "expected config to be an array of length 3, found 2");
}

#[test]
fn display_index_out_of_bounds_at_root() {
    let err = NodeError::index_out_of_bounds(&KeyPath::new(), 5, 3);
    assert_eq!(err.to_string(), "index 5 is out of bounds, config has 3 elements");
}

// ------------------------------------------------------------------------------------------ //
// Debug - error chain rendering

#[test]
fn debug_without_source() {
    let err = NodeError::new("top-level error");
    assert_eq!(format!("{err:?}"), "top-level error");
}

#[test]
fn debug_with_single_cause() {
    let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let err = NodeError::with_context("failed to load config", inner);
    assert_eq!(format!("{err:?}"), "failed to load config\n\nCaused by:\n    file not found");
}

#[test]
fn debug_with_multiple_causes() {
    let innermost = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
    let middle = NodeError::with_context("failed to read file", innermost);
    let outer = NodeError::with_context_boxed("failed to load config", Box::new(middle));
    assert_eq!(
        format!("{outer:?}"),
        "failed to load config\n\nCaused by:\n    0: failed to read file\n    1: permission denied"
    );
}

// ------------------------------------------------------------------------------------------ //
// kind_name

#[test]
fn kind_name_reports_leaf_type() {
    let node = Node::new_leaf(KeyPath::new(), Value::Bool(true));
    let err = NodeError::kind_mismatch(&KeyPath::new(), "string", &node.kind);
    assert_eq!(err.to_string(), "expected string for config, found type bool");
}

fn path(s: &str) -> KeyPath {
    s.parse().unwrap()
}
