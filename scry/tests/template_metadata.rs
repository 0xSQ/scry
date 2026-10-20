//! Covers lazy, conservative metadata for automatically generated configuration templates.
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};

use scry::desc::{DefaultDesc, DescKind, FieldDesc, VariantDesc};
use scry::{Config, Desc, Describe, Node};

// ---------------------------------------------------------------------------------------------- //

static DEFAULT_CALLS: AtomicUsize = AtomicUsize::new(0);

#[derive(Config)]
struct Settings {
    required: String,
    #[scry(default = counted_default())]
    count: u32,
    #[scry(default = "hello".to_owned())]
    label: String,
    #[scry(default = Some(3.5))]
    threshold: Option<f64>,
    absent: Option<ReadOnly>,
    #[scry(default = None)]
    explicitly_absent: Option<ReadOnly>,
    #[scry(default = Vec::new())]
    empty: Vec<ReadOnly>,
    #[scry(default = vec![])]
    another_empty: Vec<ReadOnly>,
    #[scry(default = read_only_default())]
    custom: ReadOnly,
    #[scry(from_defaults)]
    nested: Nested,
    #[scry(from_defaults)]
    mode: Mode,
}

#[derive(Config)]
struct ReadOnly {
    name: String,
}

#[derive(Config)]
struct Nested {
    #[scry(rename = "dotted.key", default = [1, 2])]
    coordinates: [u32; 2],
    #[scry(default = (false, "value".to_string()))]
    pair: (bool, String),
}

#[derive(Config)]
enum Mode {
    #[scry(default)]
    Automatic,
    Manual,
}

#[derive(Describe)]
enum DescribedChoices {
    #[scry(default)]
    First,
    Second,
}

#[derive(Config)]
struct ModeWrapper(Mode);

impl scry::FromDefaults for ModeWrapper {
    fn from_defaults_at(_path: &scry::KeyPath) -> Result<Self, scry::NodeError> {
        Ok(Self(Mode::Manual))
    }
}

#[derive(Config)]
struct WrappedDefaults {
    #[scry(from_defaults)]
    mode: ModeWrapper,
}

fn counted_default() -> u32 {
    DEFAULT_CALLS.fetch_add(1, Ordering::SeqCst);
    42
}

fn read_only_default() -> ReadOnly {
    ReadOnly {
        name: "custom".to_string(),
    }
}

#[test]
fn descriptions_keep_default_expressions_lazy() {
    let before = DEFAULT_CALLS.load(Ordering::SeqCst);
    let description = Settings::describe();
    assert_eq!(DEFAULT_CALLS.load(Ordering::SeqCst), before);
    let _ = description.display();
    assert_eq!(DEFAULT_CALLS.load(Ordering::SeqCst), before);

    let node = field(&description, "count").template_default().unwrap().unwrap();
    assert_eq!(node.as_type::<u32>().unwrap(), 42);
    assert_eq!(DEFAULT_CALLS.load(Ordering::SeqCst), before + 1);
}

#[test]
fn known_defaults_use_values_without_parsing_display_text() {
    let description = Settings::describe();
    let label = field(&description, "label");
    assert!(label.default_display().is_none());
    assert_eq!(label.template_default().unwrap().unwrap().as_type::<String>().unwrap(), "hello");
    assert_eq!(
        field(&description, "threshold")
            .template_default()
            .unwrap()
            .unwrap()
            .as_type::<Option<f64>>()
            .unwrap(),
        Some(3.5),
    );
    assert!(!field(&description, "required").is_optional());
    assert!(field(&description, "required").template_default().unwrap().is_none());
    assert!(field(&description, "custom").is_optional());
    assert!(field(&description, "custom").template_default().unwrap().is_none());
    assert!(description.template_default().unwrap().is_none());
}

#[test]
fn null_and_empty_defaults_do_not_require_writable_element_types() {
    let description = Settings::describe();
    for name in ["absent", "explicitly_absent"] {
        let value = field(&description, name).template_default().unwrap().unwrap();
        assert!(value.as_type::<Option<String>>().unwrap().is_none());
    }
    for name in ["empty", "another_empty"] {
        let value = field(&description, name).template_default().unwrap().unwrap();
        assert!(value.as_type::<Vec<String>>().unwrap().is_empty());
    }
}

#[test]
fn recursive_defaults_preserve_literal_keys_and_enum_encoding() {
    let description = Settings::describe();
    let nested = field(&description, "nested").template_default().unwrap().unwrap();
    assert_eq!(nested.req::<[u32; 2]>(["dotted.key"]).unwrap(), [1, 2]);
    assert_eq!(nested.req::<(bool, String)>("pair").unwrap(), (false, "value".to_string()));

    let mode = field(&description, "mode").template_default().unwrap().unwrap();
    assert_eq!(mode.as_type::<String>().unwrap(), "automatic");
    assert!(Mode::describe().template_default().unwrap().is_some());
    assert!(Nested::describe().template_default().unwrap().is_some());
}

#[test]
fn enum_shape_does_not_establish_a_concrete_type_default() {
    let manual = Desc::enumeration(vec![VariantDesc::unit("first")]);
    assert!(manual.type_default.is_none());

    let description = DescribedChoices::describe();
    assert_eq!(description.unit_enum_variants().unwrap()[0].name, "first");
    assert!(description.type_default.is_some());
    assert!(description.template_default().unwrap().is_none());
}

#[test]
fn transparent_wrappers_do_not_inherit_the_inner_types_default_policy() {
    let wrapper = ModeWrapper::describe();
    assert!(wrapper.unit_enum_variants().is_some());
    assert!(wrapper.type_default.is_none());

    let description = WrappedDefaults::describe();
    assert!(field(&description, "mode").is_optional());
    assert!(field(&description, "mode").template_default().unwrap().is_none());
    assert!(description.template_default().unwrap().is_none());
    let decoded = scry::Node::empty_map().as_type::<WrappedDefaults>().unwrap();
    assert!(matches!(decoded.mode.0, Mode::Manual));
}

#[test]
fn built_in_wrappers_preserve_enum_shape_without_advertising_inner_defaults() {
    assert!(Mode::describe().type_default.is_some());
    for description in [
        <&Mode>::describe(),
        <&mut Mode>::describe(),
        Box::<Mode>::describe(),
        std::rc::Rc::<Mode>::describe(),
        std::sync::Arc::<Mode>::describe(),
        Option::<Mode>::describe(),
    ] {
        assert!(description.type_default.is_none());
        let variants = description.unit_enum_variants().unwrap();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].name, "automatic");
        assert_eq!(variants[1].name, "manual");
    }
    assert!(Option::<Mode>::describe().nullable);
    assert!(!Box::<Mode>::describe().nullable);
}

#[test]
fn adapted_shapes_do_not_supply_the_targets_default_policy() {
    #[derive(Config)]
    struct AdaptedDefaults {
        #[scry(from_defaults, describe_with(Mode::describe))]
        mode: ModeWrapper,
    }

    let description = AdaptedDefaults::describe();
    let mode = field(&description, "mode");
    assert!(mode.is_optional());
    assert!(mode.default_display().is_none());
    assert!(mode.template_default().unwrap().is_none());
    let decoded = Node::empty_map().as_type::<AdaptedDefaults>().unwrap();
    assert!(matches!(decoded.mode.0, Mode::Manual));
}

#[test]
fn handwritten_default_errors_retain_the_nested_field_path() {
    let child = Desc::structure(vec![FieldDesc::new("bad.key", Desc::plain("string"))
        .with_fallback(DefaultDesc::unknown().with_value(|| {
            Err(scry::NodeError::invalid_value(&scry::KeyPath::new(), "bad default"))
        }))]);
    let error = child.field_defaults().unwrap_err();
    assert!(error.to_string().contains("bad.key"));
}

fn field<'a>(description: &'a Desc, name: &str) -> &'a FieldDesc {
    let DescKind::Struct { fields } = &description.kind else {
        panic!("expected named fields");
    };
    fields.iter().find(|field| field.name == name).unwrap()
}
