//! Integration tests for enum derive macros.
#![cfg(feature = "format-json")]

use scry::desc::{DescKind, VariantRepr};
use scry::node::{Format, Kind};
use scry::{Describe, FromNode, Node, ToNode};

// ---------------------------------------------------------------------------------------------- //
// Test Enums

/// Test enum covering all variant types.
#[derive(Debug, Clone, PartialEq, FromNode, ToNode, Describe)]
enum TestEnum {
    /// A unit variant.
    Unit,
    /// Single value tuple.
    Single(String),
    /// Two value tuple.
    Pair(String, i32),
    /// Struct variant with named fields.
    Named { name: String, count: i32 },
}

/// Test enum for struct variant field semantics.
#[derive(Debug, Clone, PartialEq, FromNode, ToNode, Describe)]
enum StructFieldsEnum {
    /// Struct variant with optional and default fields.
    Config {
        required: String,
        optional: Option<i32>,
        #[scry(default = 10)]
        with_default: i32,
    },
}

/// Test enum with renamed variants and fields.
#[derive(Debug, Clone, PartialEq, FromNode, ToNode, Describe)]
enum RenamedEnum {
    /// Unit variant with custom name.
    #[scry(rename = "CustomUnit")]
    UnitVariant,
    /// Struct variant with renamed fields.
    #[scry(rename = "custom_struct")]
    StructVariant {
        #[scry(rename = "req")]
        required_field: String,
        #[scry(rename = "opt")]
        optional_field: Option<i32>,
    },
}

/// Test enum for newtype variants with array-expecting inner types.
#[derive(Debug, Clone, PartialEq, FromNode, ToNode, Describe)]
enum NewtypeArrayEnum {
    /// Newtype with Vec inner type - array payload is valid.
    Items(Vec<i32>),
    /// Newtype with tuple inner type - array payload is valid.
    Coordinates((String, i32)),
    /// Newtype with Option<Vec> - array payload is valid.
    OptionalItems(Option<Vec<String>>),
}

/// Test enum with compound default names.
#[derive(Debug, Clone, PartialEq, FromNode, ToNode, Describe)]
enum CompoundEnum {
    RowMajor,
    ColMajor,
}

/// Test enum with enum-level rename_all casing.
#[derive(Debug, Clone, PartialEq, FromNode, ToNode, Describe)]
#[scry(rename_all = "kebab-case")]
enum KebabEnum {
    RowMajor,
    ColMajor,
}

fn node(json: &str) -> Node {
    Node::parse_str(json, Format::Json).unwrap()
}

// ---------------------------------------------------------------------------------------------- //
// Decoding

#[test]
fn unit_variant_from_string() {
    let n = node(r#""unit""#);
    let e: TestEnum = n.as_type().unwrap();
    assert_eq!(e, TestEnum::Unit);
    assert_wire(&e, r#""unit""#);
}

#[test]
fn unit_variant_case_insensitive() {
    let n = node(r#""UNIT""#);
    let e: TestEnum = n.as_type().unwrap();
    assert_eq!(e, TestEnum::Unit);
}

#[test]
fn single_tuple_from_map() {
    let n = node(r#"{"single": "hello"}"#);
    let e: TestEnum = n.as_type().unwrap();
    assert_eq!(e, TestEnum::Single("hello".into()));
    assert_wire(&e, r#"{"single": "hello"}"#);
}

#[test]
fn pair_tuple_from_map_array() {
    let n = node(r#"{"pair": ["hello", 42]}"#);
    let e: TestEnum = n.as_type().unwrap();
    assert_eq!(e, TestEnum::Pair("hello".into(), 42));
    assert_wire(&e, r#"{"pair": ["hello", 42]}"#);
}

#[test]
fn struct_variant_from_nested_map() {
    let n = node(r#"{"named": {"name": "alice", "count": 5}}"#);
    let e: TestEnum = n.as_type().unwrap();
    assert_eq!(
        e,
        TestEnum::Named {
            name: "alice".into(),
            count: 5,
        }
    );
    assert_wire(&e, r#"{"named": {"name": "alice", "count": 5}}"#);
}

// ---------------------------------------------------------------------------------------------- //
// Error Cases

#[test]
fn unknown_variant_errors() {
    let n = node(r#""unknown""#);
    let err = n.as_type::<TestEnum>().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unknown variant 'unknown'"));
    assert!(msg.contains("expected one of"));
    assert!(msg.contains("unit"));
    assert!(msg.contains("single"));
    assert!(msg.contains("pair"));
    assert!(msg.contains("named"));
}

#[test]
fn unknown_map_variant_errors_list_expected_variants() {
    let n = node(r#"{"unknown": {}}"#);
    let err = n.as_type::<TestEnum>().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unknown variant 'unknown'"));
    assert!(msg.contains("expected one of"));
    assert!(msg.contains("unit"));
    assert!(msg.contains("single"));
    assert!(msg.contains("pair"));
    assert!(msg.contains("named"));
}

#[test]
fn pair_wrong_arity_errors() {
    let n = node(r#"{"pair": ["hello"]}"#);
    assert!(n.as_type::<TestEnum>().is_err());
}

#[test]
fn pair_too_many_elements_errors() {
    let n = node(r#"{"pair": ["hello", 42, true]}"#);
    assert!(n.as_type::<TestEnum>().is_err());
}

#[test]
fn struct_missing_field_errors() {
    let n = node(r#"{"named": {"name": "alice"}}"#);
    assert!(n.as_type::<TestEnum>().is_err());
}

// ---------------------------------------------------------------------------------------------- //
// Variant descriptions

#[test]
fn desc_includes_all_variants() {
    let desc = TestEnum::describe();
    let variants = match &desc.kind {
        DescKind::Enum { variants } => variants,
        _ => panic!("Expected Enum kind"),
    };
    assert_eq!(variants.len(), 4);

    // Check variant names
    let names: Vec<&str> = variants.iter().map(|v| v.name.as_str()).collect();
    assert!(names.contains(&"unit"));
    assert!(names.contains(&"single"));
    assert!(names.contains(&"pair"));
    assert!(names.contains(&"named"));
}

#[test]
fn struct_variant_desc_has_nested_fields() {
    let desc = TestEnum::describe();
    let variants = match &desc.kind {
        DescKind::Enum { variants } => variants,
        _ => panic!("Expected Enum kind"),
    };
    let named_variant = variants.iter().find(|v| v.name == "named").unwrap();

    // Should have a nested payload with fields
    let payload = match &named_variant.repr {
        VariantRepr::Payload { payload, .. } => payload,
        _ => panic!("Expected Payload variant"),
    };
    let inner_fields = match &payload.kind {
        DescKind::Struct { fields, .. } => fields,
        _ => panic!("Expected Struct kind in payload"),
    };
    assert_eq!(inner_fields.len(), 2);

    let inner_names: Vec<&str> = inner_fields.iter().map(|f| f.name.as_str()).collect();
    assert!(inner_names.contains(&"name"));
    assert!(inner_names.contains(&"count"));
}

#[test]
fn compound_enum_parses_snake_case() {
    let n = node(r#""row_major""#);
    let e: CompoundEnum = n.as_type().unwrap();
    assert_eq!(e, CompoundEnum::RowMajor);
}

#[test]
fn compound_enum_parses_kebab_case_alias() {
    let n = node(r#""row-major""#);
    let e: CompoundEnum = n.as_type().unwrap();
    assert_eq!(e, CompoundEnum::RowMajor);
}

#[test]
fn compound_enum_serializes_snake_case() {
    let node = CompoundEnum::ColMajor.to_node().unwrap();
    match node.kind {
        Kind::Leaf(leaf) => match leaf.value {
            scry::node::Value::String(value) => assert_eq!(value, "col_major"),
            _ => panic!("Expected string leaf"),
        },
        _ => panic!("Expected leaf"),
    }
}

#[test]
fn compound_enum_desc_uses_snake_case() {
    let desc = CompoundEnum::describe();
    let variants = match &desc.kind {
        DescKind::Enum { variants } => variants,
        _ => panic!("Expected Enum kind"),
    };
    let names: Vec<&str> = variants.iter().map(|v| v.name.as_str()).collect();
    assert!(names.contains(&"row_major"));
    assert!(names.contains(&"col_major"));
}

#[test]
fn enum_rename_all_parses_kebab_case() {
    let n = node(r#""row-major""#);
    let e: KebabEnum = n.as_type().unwrap();
    assert_eq!(e, KebabEnum::RowMajor);
}

#[test]
fn enum_rename_all_serializes_kebab_case() {
    let node = KebabEnum::ColMajor.to_node().unwrap();
    match node.kind {
        Kind::Leaf(leaf) => match leaf.value {
            scry::node::Value::String(value) => assert_eq!(value, "col-major"),
            _ => panic!("Expected string leaf"),
        },
        _ => panic!("Expected leaf"),
    }
}

#[test]
fn enum_rename_all_desc_uses_kebab_case() {
    let desc = KebabEnum::describe();
    let variants = match &desc.kind {
        DescKind::Enum { variants } => variants,
        _ => panic!("Expected Enum kind"),
    };
    let names: Vec<&str> = variants.iter().map(|v| v.name.as_str()).collect();
    assert!(names.contains(&"row-major"));
    assert!(names.contains(&"col-major"));
}

// ---------------------------------------------------------------------------------------------- //
// Strictness Tests: Map Form

#[test]
fn empty_map_errors() {
    let n = node(r#"{}"#);
    let err = n.as_type::<TestEnum>().unwrap_err();
    assert!(err.to_string().contains("empty map"));
}

#[test]
fn multiple_variant_keys_errors() {
    let n = node(r#"{"single": "hello", "pair": ["a", 1]}"#);
    let err = n.as_type::<TestEnum>().unwrap_err();
    assert!(err.to_string().contains("exactly one"));
}

// ---------------------------------------------------------------------------------------------- //
// Strictness Tests: Unit Variants

#[test]
fn unit_variant_as_map_errors() {
    // Unit variants must use string form, not {"unit": ...}
    let n = node(r#"{"unit": null}"#);
    let err = n.as_type::<TestEnum>().unwrap_err();
    assert!(err.to_string().contains("must be written as a string"));
}

// ---------------------------------------------------------------------------------------------- //
// Strictness Tests: Payload Variants

#[test]
fn payload_variant_as_string_errors() {
    // Payload variants cannot use string form
    let n = node(r#""single""#);
    let err = n.as_type::<TestEnum>().unwrap_err();
    assert!(err.to_string().contains("requires a payload"));
}

// ---------------------------------------------------------------------------------------------- //
// Strictness Tests: Other Invalid Forms

#[test]
fn array_form_errors() {
    let n = node(r#"["unit"]"#);
    let err = n.as_type::<TestEnum>().unwrap_err();
    assert!(err.to_string().contains("found type array"));
}

#[test]
fn number_form_errors() {
    let n = node(r#"42"#);
    let err = n.as_type::<TestEnum>().unwrap_err();
    assert!(err.to_string().contains("found type i64"));
}

// ---------------------------------------------------------------------------------------------- //
// Struct Variant Field Semantics Tests

#[test]
fn struct_variant_optional_field_missing_is_none() {
    let n = node(r#"{"config": {"required": "hello"}}"#);
    let e: StructFieldsEnum = n.as_type().unwrap();
    assert_eq!(
        e,
        StructFieldsEnum::Config {
            required: "hello".into(),
            optional: None,
            with_default: 10,
        }
    );
}

#[test]
fn struct_variant_optional_field_present() {
    let n = node(r#"{"config": {"required": "hello", "optional": 42}}"#);
    let e: StructFieldsEnum = n.as_type().unwrap();
    assert_eq!(
        e,
        StructFieldsEnum::Config {
            required: "hello".into(),
            optional: Some(42),
            with_default: 10,
        }
    );
}

#[test]
fn struct_variant_default_field_overridden() {
    let n = node(r#"{"config": {"required": "hello", "with_default": 99}}"#);
    let e: StructFieldsEnum = n.as_type().unwrap();
    assert_eq!(
        e,
        StructFieldsEnum::Config {
            required: "hello".into(),
            optional: None,
            with_default: 99,
        }
    );
}

#[test]
fn struct_variant_unknown_key_errors() {
    let n = node(r#"{"config": {"required": "hello", "unknown": 123}}"#);
    let err = n.as_type::<StructFieldsEnum>().unwrap_err();
    assert!(err.to_string().contains("unknown"));
}

// ---------------------------------------------------------------------------------------------- //
// Encoding Optional Values

#[test]
fn struct_variant_serialization_preserves_optional_values_and_explicit_defaults() {
    for (optional, expected) in [
        (None, r#"{"config": {"required": "hello", "optional": null, "with_default": 10}}"#),
        (Some(42), r#"{"config": {"required": "hello", "optional": 42, "with_default": 10}}"#),
    ] {
        assert_wire(
            &StructFieldsEnum::Config {
                required: "hello".into(),
                optional,
                with_default: 10,
            },
            expected,
        );
    }
}
// ---------------------------------------------------------------------------------------------- //
// Rename Tests

#[test]
fn renamed_unit_variant_case_insensitive() {
    // Should match "customunit" (lowercased) against renamed key "CustomUnit"
    let n = node(r#""customunit""#);
    let e: RenamedEnum = n.as_type().unwrap();
    assert_eq!(e, RenamedEnum::UnitVariant);
}

#[test]
fn renamed_unit_variant_original_case() {
    let n = node(r#""CustomUnit""#);
    let e: RenamedEnum = n.as_type().unwrap();
    assert_eq!(e, RenamedEnum::UnitVariant);
}

#[test]
fn renamed_struct_variant_from_node() {
    // Uses renamed variant key and renamed field keys
    let n = node(r#"{"custom_struct": {"req": "hello"}}"#);
    let e: RenamedEnum = n.as_type().unwrap();
    assert_eq!(
        e,
        RenamedEnum::StructVariant {
            required_field: "hello".into(),
            optional_field: None,
        }
    );
}

#[test]
fn renamed_struct_variant_accepts_kebab_case_alias() {
    let n = node(r#"{"custom-struct": {"req": "hello"}}"#);
    let e: RenamedEnum = n.as_type().unwrap();
    assert_eq!(
        e,
        RenamedEnum::StructVariant {
            required_field: "hello".into(),
            optional_field: None,
        }
    );
}

#[test]
fn renamed_struct_variant_to_node_uses_renamed_keys() {
    let value = RenamedEnum::StructVariant {
        required_field: "hello".into(),
        optional_field: Some(42),
    };
    assert_wire(&value, r#"{"custom_struct": {"req": "hello", "opt": 42}}"#);
}
// ---------------------------------------------------------------------------------------------- //
// Newtype Array Payload Tests

#[test]
fn newtype_vec_accepts_array_payload() {
    let n = node(r#"{"items": [1, 2, 3]}"#);
    let e: NewtypeArrayEnum = n.as_type().unwrap();
    assert_eq!(e, NewtypeArrayEnum::Items(vec![1, 2, 3]));
    assert_wire(&e, r#"{"items": [1, 2, 3]}"#);
}

#[test]
fn newtype_tuple_accepts_array_payload() {
    let n = node(r#"{"coordinates": ["origin", 42]}"#);
    let e: NewtypeArrayEnum = n.as_type().unwrap();
    assert_eq!(e, NewtypeArrayEnum::Coordinates(("origin".into(), 42)));
    assert_wire(&e, r#"{"coordinates": ["origin", 42]}"#);
}

#[test]
fn newtype_option_vec_accepts_array_payload() {
    let n = node(r#"{"optional_items": ["a", "b"]}"#);
    let e: NewtypeArrayEnum = n.as_type().unwrap();
    assert_eq!(e, NewtypeArrayEnum::OptionalItems(Some(vec!["a".into(), "b".into()])));
    assert_wire(&e, r#"{"optional_items": ["a", "b"]}"#);
}

fn assert_wire(value: &impl ToNode, expected: &str) {
    assert_eq!(
        value.to_node().unwrap().to_string_as(Format::Json).unwrap(),
        node(expected).to_string_as(Format::Json).unwrap()
    );
}
