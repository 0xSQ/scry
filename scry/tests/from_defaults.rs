//! Tests for `scry::from_defaults` and `Node::empty_map`.

use scry::{Config, Node};

#[derive(Debug, PartialEq, Config)]
struct Settings {
    #[scry(default = 1.25)]
    zoom: f32,
    #[scry(default = true)]
    enabled: bool,
    name: Option<String>,
}

#[test]
fn from_defaults_uses_field_attributes() {
    // No `use scry::FromNode` needed: the free function carries the bound.
    let settings: Settings = scry::from_defaults().unwrap();
    assert_eq!(
        settings,
        Settings {
            zoom: 1.25,
            enabled: true,
            name: None,
        }
    );
}

#[test]
fn empty_map_deserializes_to_defaults() {
    // The same thing `from_defaults` does, spelled out via the new primitive.
    let settings: Settings = Node::empty_map().as_type().unwrap();
    assert_eq!(settings.zoom, 1.25);
    assert!(settings.enabled);
    assert_eq!(settings.name, None);
}

// Fields exist only to drive the missing-required-field error path; never constructed or read.
#[allow(dead_code)]
#[derive(Debug, Config)]
struct NeedsHost {
    #[scry(default = 3)]
    retries: u32,
    host: String, // required, no default
}

#[test]
fn from_defaults_errors_on_required_field_at_its_path() {
    let err = scry::from_defaults::<NeedsHost>().unwrap_err();
    let message = err.to_string();
    assert!(message.contains("host"), "error should name the missing field's path, got: {message}");
}
