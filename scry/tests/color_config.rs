//! Checks native colors and standard adapters across configuration operations.

use std::error::Error;
use std::num::{ParseIntError, TryFromIntError};

use scry::convert::{read, write};
use scry::desc::{DescKind, FieldDesc};
use scry::kit::color::{
    named, rgb8_array, rgb8_tuple, rgba8_array, rgba8_tuple, NonOpaqueError, ParseError, Rgb8,
    Rgba8,
};
use scry::node::Format;
use scry::writer::{format_template, Document, RhaiWriter, TemplateMode};
use scry::{Config, Desc, Describe, FromNode, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, PartialEq, Config, ToNode)]
struct Appearance {
    /// The opaque canvas background.
    #[scry(default = Rgb8::new(24, 24, 28))]
    background: Rgb8,
    /// The overlay color, including opacity.
    overlay: Rgba8,
    /// Optional colors in their authored order.
    palette: Vec<Option<Rgba8>>,
    /// An optional opaque selection color.
    selection: Option<Rgb8>,
}

#[derive(Debug, PartialEq, Config, ToNode)]
struct Adapted {
    #[scry(with(rgb8_array))]
    rgb_array: [u8; 3],
    #[scry(with(rgba8_array))]
    rgba_array: [u8; 4],
    #[scry(with(rgb8_tuple))]
    rgb_tuple: (u8, u8, u8),
    #[scry(with(rgba8_tuple))]
    rgba_tuple: (u8, u8, u8, u8),
}

#[derive(Debug, PartialEq, Config, ToNode)]
struct AdaptedPalette {
    #[scry(
        from_node_with(|node| read::vec(node, |node| read::option(node, rgba8_array::from_node))),
        to_node_with(|values| write::list(values, |value| write::option(value, rgba8_array::to_node))),
        describe_with(|| Desc::list(rgba8_array::describe().nullable()))
    )]
    colors: Vec<Option<[u8; 4]>>,
}

#[derive(Debug, PartialEq, Config, ToNode)]
struct Defaulted {
    #[scry(default = Rgb8::new(32, 128, 224))]
    native: Rgb8,
    #[scry(default = [32, 128, 224], with(rgb8_array))]
    adapted: [u8; 3],
}

#[test]
fn named_constants_work_as_native_defaults_and_write_canonical_colors() {
    #[derive(Debug, PartialEq, Config, ToNode)]
    struct NamedDefaults {
        #[scry(default = named::TEAL)]
        background: Rgb8,
        #[scry(default = Rgba8::from(named::REBECCAPURPLE))]
        selection: Rgba8,
    }

    let settings: NamedDefaults = scry::from_defaults().unwrap();
    assert_eq!(settings.background.into_array(), [0, 128, 128]);
    assert_eq!(settings.selection.into_array(), [102, 51, 153, 255]);
    let output = settings.to_node().unwrap();
    assert_eq!(output.req::<String>("background").unwrap(), "#008080");
    assert_eq!(output.req::<String>("selection").unwrap(), "#663399");
    assert_eq!(output.as_type::<NamedDefaults>().unwrap(), settings);
}

#[test]
fn equivalent_inputs_share_native_and_standard_adapter_behavior() {
    for source in [
        "\"teal\"",
        "\"#008080\"",
        "\"#008080ff\"",
        "[0, 128, 128]",
        "[0, 128, 128, 255]",
        "[\"0\", \"128\", \"128\", \"255\"]",
        "#{ r: 0, g: 128, b: 128 }",
        "#{ r: 0, g: 128, b: 128, a: 255 }",
    ] {
        let input = node(source);
        assert_eq!(Rgb8::from_node(&input).unwrap(), Rgb8::new(0, 128, 128), "{source}");
        assert_eq!(Rgba8::from_node(&input).unwrap(), Rgba8::new(0, 128, 128, 255), "{source}");
        assert_eq!(rgb8_array::from_node(&input).unwrap(), [0, 128, 128], "{source}");
        assert_eq!(rgba8_array::from_node(&input).unwrap(), [0, 128, 128, 255], "{source}");
        assert_eq!(rgb8_tuple::from_node(&input).unwrap(), (0, 128, 128), "{source}");
        assert_eq!(rgba8_tuple::from_node(&input).unwrap(), (0, 128, 128, 255), "{source}");
        input.ensure_no_unknown_keys().unwrap();
    }
    for source in [
        "\"#2080e040\"",
        "[32, 128, 224, 64]",
        "#{ r: 32, g: 128, b: 224, a: 64 }",
    ] {
        let input = node(source);
        assert_eq!(Rgba8::from_node(&input).unwrap().into_array(), [32, 128, 224, 64]);
        assert_eq!(rgba8_array::from_node(&input).unwrap(), [32, 128, 224, 64]);
        assert_eq!(rgba8_tuple::from_node(&input).unwrap(), (32, 128, 224, 64));
        assert!(Rgb8::from_node(&input).is_err());
        assert!(rgb8_array::from_node(&input).is_err());
        assert!(rgb8_tuple::from_node(&input).is_err());
        input.ensure_no_unknown_keys().unwrap();
    }
}

#[test]
fn direct_native_readers_consume_every_accepted_leaf() {
    for source in [
        "\"#2080e040\"",
        "[32, 128, 224]",
        "[32, 128, 224, 64]",
        "#{ r: \"32\", g: 128, b: 224, a: \"64\" }",
    ] {
        let input = node(source);
        Rgba8::from_node(&input).unwrap();
        input.ensure_no_unknown_keys().unwrap();
    }
    for source in [
        "\"teal\"",
        "[0, 128, 128]",
        "#{ r: 0, g: 128, b: 128, a: 255 }",
    ] {
        let input = node(source);
        Rgb8::from_node(&input).unwrap();
        input.ensure_no_unknown_keys().unwrap();
    }
}

#[test]
fn native_colors_keep_container_order_nulls_and_missing_only_defaults() {
    let input = node(indoc::indoc! {r##"
        #{
            overlay: [255, 0, 0, 0],
            palette: ["teal", (), #{ r: 32, g: 128, b: 224, a: 64 }, "teal"],
        }
    "##});
    let appearance = Appearance::from_node(&input).unwrap();
    assert_eq!(appearance.background, Rgb8::new(24, 24, 28));
    assert_eq!(appearance.overlay, Rgba8::new(255, 0, 0, 0));
    assert_eq!(appearance.selection, None);
    assert_eq!(
        appearance.palette,
        vec![
            Some(Rgba8::new(0, 128, 128, 255)),
            None,
            Some(Rgba8::new(32, 128, 224, 64)),
            Some(Rgba8::new(0, 128, 128, 255)),
        ]
    );
    input.ensure_no_unknown_keys().unwrap();

    let output = appearance.to_node().unwrap();
    assert_eq!(output.req::<String>("background").unwrap(), "#18181c");
    assert_eq!(output.req::<String>("overlay").unwrap(), "#ff000000");
    assert_eq!(
        output.req::<Vec<Option<String>>>("palette").unwrap(),
        vec![
            Some("#008080".to_string()),
            None,
            Some("#2080e040".to_string()),
            Some("#008080".to_string()),
        ]
    );
    assert_eq!(Appearance::from_node(&output).unwrap(), appearance);
    output.ensure_no_unknown_keys().unwrap();

    let input = node("#{ background: (), overlay: \"red\", palette: [] }");
    let error = Appearance::from_node(&input).unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_keys(["background"])));
    let error = Appearance::from_node(&node("#{ palette: [] }")).unwrap_err();
    assert!(
        matches!(error, NodeError::MissingRequired { path } if path == KeyPath::from_keys(["overlay"]))
    );
}

#[test]
fn standard_adapters_write_canonical_hex_and_compose_through_existing_helpers() {
    let input = node(indoc::indoc! {r##"
        #{
            rgb_array: "#abc",
            rgba_array: [255, 0, 0, 0],
            rgb_tuple: #{ r: 0, g: 128, b: 128 },
            rgba_tuple: "#1234",
        }
    "##});
    let adapted = Adapted::from_node(&input).unwrap();
    assert_eq!(adapted.rgb_array, [170, 187, 204]);
    assert_eq!(adapted.rgba_array, [255, 0, 0, 0]);
    assert_eq!(adapted.rgb_tuple, (0, 128, 128));
    assert_eq!(adapted.rgba_tuple, (17, 34, 51, 68));
    input.ensure_no_unknown_keys().unwrap();
    let output = adapted.to_node().unwrap();
    for (key, expected) in [
        ("rgb_array", "#aabbcc"),
        ("rgba_array", "#ff000000"),
        ("rgb_tuple", "#008080"),
        ("rgba_tuple", "#11223344"),
    ] {
        assert_eq!(output.req::<String>(key).unwrap(), expected);
    }
    assert_eq!(Adapted::from_node(&output).unwrap(), adapted);

    let input = node("#{ colors: [(), [255, 0, 0, 0], \"teal\"] }");
    let colors = AdaptedPalette::from_node(&input).unwrap();
    assert_eq!(colors.colors, vec![None, Some([255, 0, 0, 0]), Some([0, 128, 128, 255])]);
    input.ensure_no_unknown_keys().unwrap();
    assert_eq!(AdaptedPalette::from_node(&colors.to_node().unwrap()).unwrap(), colors);

    assert!(node("\"red\"").as_type::<[u8; 3]>().is_err());
    assert!(node("\"red\"").as_type::<(u8, u8, u8)>().is_err());
}

#[test]
fn invalid_shapes_ranges_and_null_channels_are_rejected() {
    for source in [
        "()",
        "true",
        "32",
        "0.5",
        "[]",
        "[1]",
        "[1, 2]",
        "[1, 2, 3, 4, 5]",
        "[1.0, 2, 3]",
        "[1, -1, 3]",
        "[1, 256, 3]",
        "[1, (), 3]",
        "[1, 2, 3, ()]",
        "#{ r: 1, b: 3 }",
        "#{ r: 1, g: 2, b: 3, a: () }",
        "#{ r: 1, g: 2, b: 3, a: 0.5 }",
    ] {
        let input = node(source);
        assert!(Rgb8::from_node(&input).is_err(), "accepted RGB {source}");
        assert!(Rgba8::from_node(&input).is_err(), "accepted RGBA {source}");
        assert!(rgb8_array::from_node(&input).is_err(), "accepted RGB array {source}");
        assert!(rgba8_tuple::from_node(&input).is_err(), "accepted RGBA tuple {source}");
    }
}

#[test]
fn strict_map_readers_reject_unknown_keys_even_when_values_are_empty_containers() {
    for extra in ["7", "[]", "#{}"] {
        let input =
            node(&format!("#{{ scope: #{{ color: #{{ r: 1, g: 2, b: 3, opacity: {extra} }} }} }}"));
        let color = input.req_node("scope.color").unwrap();
        for error in [
            Rgb8::from_node(color).unwrap_err(),
            Rgba8::from_node(color).unwrap_err(),
        ] {
            let NodeError::UnknownKeys { paths } = error else {
                panic!("expected unknown-key validation, found {error:?}");
            };
            assert_eq!(paths, vec![KeyPath::from_keys(["scope", "color", "opacity"])]);
        }
    }
}

#[test]
fn nested_channel_failures_keep_full_paths_and_native_conversion_causes() {
    let input = node("#{ scope: #{ colors: [(), #{ r: 1, g: \"bad\", b: 3 }] } }");
    let error = AdaptedPalette::from_node(input.req_node("scope").unwrap()).unwrap_err();
    let expected = KeyPath::from_keys(["scope", "colors"]).push_index(1).push_key("g");
    assert_eq!(error.path(), Some(&expected));
    cause::<ParseIntError>(&error);

    let input = node("#{ scope: #{ colors: [[1, 256, 3]] } }");
    let error = AdaptedPalette::from_node(input.req_node("scope").unwrap()).unwrap_err();
    let expected = KeyPath::from_keys(["scope", "colors"]).push_index(0).push_index(1);
    assert_eq!(error.path(), Some(&expected));
    cause::<TryFromIntError>(&error);

    let input = node("#{ scope: #{ color: \"unknown-color\" } }");
    let color = input.req_node("scope.color").unwrap();
    let error = Rgba8::from_node(color).unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_keys(["scope", "color"])));
    cause::<ParseError>(&error);
}

#[test]
fn opacity_rejection_identifies_explicit_alpha_or_the_complete_string() {
    for (source, expected) in [
        ("#{ r: 32, g: 128, b: 224, a: 64 }", KeyPath::from_keys(["scope", "color", "a"])),
        ("[32, 128, 224, 64]", KeyPath::from_keys(["scope", "color"]).push_index(3)),
        ("\"#2080e040\"", KeyPath::from_keys(["scope", "color"])),
    ] {
        let input = node(&format!("#{{ scope: #{{ color: {source} }} }}"));
        let color = input.req_node("scope.color").unwrap();
        let error = Rgb8::from_node(color).unwrap_err();
        assert_eq!(error.path(), Some(&expected), "{source}");
        assert_eq!(cause::<NonOpaqueError>(&error).alpha(), 64);
    }
}

#[test]
fn descriptions_remain_atomic_and_keep_field_and_container_metadata() {
    for description in [
        Rgb8::describe(),
        Rgba8::describe(),
        rgb8_array::describe(),
        rgba8_tuple::describe(),
    ] {
        assert!(matches!(description.kind, DescKind::Plain { .. }));
        assert!(!description.nullable);
        assert!(!description.doc.is_empty());
        assert!(description.type_default.is_none());
    }
    let appearance = Appearance::describe();
    assert!(field(&appearance, "background").is_optional());
    assert!(!field(&appearance, "overlay").is_optional());
    assert!(field(&appearance, "selection").value.nullable);
    let DescKind::List { item } = &field(&appearance, "palette").value.kind else {
        panic!("palette must retain its outer list shape");
    };
    assert!(item.nullable);
    assert!(matches!(item.kind, DescKind::Plain { .. }));
    appearance.validate_path("overlay").unwrap();
    assert!(appearance.validate_path("overlay.r").is_err());
    assert!(appearance.validate_path("overlay[0]").is_err());
    appearance.validate_path("palette[2]").unwrap();

    let defaults = Defaulted::describe();
    assert!(field(&defaults, "native").is_optional());
    assert!(field(&defaults, "adapted").is_optional());
    assert!(field(&defaults, "adapted").default_display().is_none());
    assert!(field(&defaults, "adapted").template_default().unwrap().is_none());
}

#[test]
fn generated_settings_and_templates_use_native_and_adapted_defaults_safely() {
    let settings: Defaulted = scry::from_defaults().unwrap();
    assert_eq!(settings.native.into_array(), settings.adapted);
    let document = Document::from_config(&settings).unwrap();
    let mut writer = RhaiWriter::to_string_writer();
    writer.write(&document).unwrap();
    let output = writer.into_string().unwrap();
    assert!(output.contains("native: \"#2080e0\""));
    assert!(output.contains("adapted: \"#2080e0\""));
    assert_eq!(Defaulted::from_node(&node(&output)).unwrap(), settings);

    for mode in [TemplateMode::Min, TemplateMode::Ref, TemplateMode::Explicit] {
        let output = format_template::<Defaulted>(Format::Rhai, mode).unwrap();
        assert_eq!(Defaulted::from_node(&node(&output)).unwrap(), settings);
    }

    #[cfg(feature = "format-json")]
    {
        let output = settings.to_node().unwrap().to_string_as(Format::Json).unwrap();
        let input = Node::parse_str(&output, Format::Json).unwrap();
        assert_eq!(Defaulted::from_node(&input).unwrap(), settings);
    }
}

// ---------------------------------------------------------------------------------------------- //

fn node(source: &str) -> Node {
    Node::parse_str(source, Format::Rhai).unwrap()
}

fn field<'a>(description: &'a Desc, name: &str) -> &'a FieldDesc {
    let DescKind::Struct { fields } = &description.kind else {
        panic!("expected struct description");
    };
    fields.iter().find(|field| field.name == name).unwrap()
}

fn cause<T: Error + 'static>(error: &NodeError) -> &T {
    let mut current = Some(error as &(dyn Error + 'static));
    while let Some(error) = current {
        if let Some(cause) = error.downcast_ref::<T>() {
            return cause;
        }
        current = error.source();
    }
    panic!("missing expected cause {}", std::any::type_name::<T>())
}
