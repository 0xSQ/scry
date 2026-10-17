//! Checks syntax, annotations, and failure behavior of configuration document renderers.

use std::error::Error;
use std::io::{self, Write};

use scry::kit::KeyValues;
use scry::node::Format;
use scry::writer::{Document, RhaiWriter, RhaiWriterConfig};
use scry::{Config, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[test]
fn associative_settings_preserve_entry_order_and_duplicate_keys() -> Result<(), NodeError> {
    #[derive(Debug, PartialEq, Config, ToNode)]
    struct Settings {
        /// Ordered named weights.
        weights: KeyValues<i32>,
    }

    let settings = Settings {
        weights: KeyValues::new(vec![
            ("zeta".to_owned(), 7),
            ("alpha".to_owned(), 9),
            ("zeta".to_owned(), 11),
        ]),
    };
    let document = Document::from_config(&settings)?;
    let output = render_rhai(&document)?;
    assert!(output.contains("// Ordered named weights."));
    let node = Node::parse_str(&output, Format::Rhai)?;
    assert_eq!(node.as_type::<Settings>()?, settings);

    #[cfg(feature = "format-json5")]
    {
        let mut writer = scry::writer::Json5Writer::to_string_writer();
        writer.write(&document)?;
        let node = Node::parse_str(writer.as_str()?, Format::Json5)?;
        assert_eq!(node.as_type::<Settings>()?, settings);
    }
    Ok(())
}

#[test]
fn comments_follow_required_commas_without_forcing_a_trailing_comma() -> Result<(), NodeError> {
    let document = Document::map(|map| {
        map.entry("first", 1)?.comment("First.").end_comment("Keep.");
        map.entry("last", 2)?.blank_before();
        Ok(())
    })?;
    let mut writer = RhaiWriter::with_config(
        Vec::new(),
        RhaiWriterConfig {
            trailing_commas: false,
            ..Default::default()
        },
    );
    writer.write(&document)?;
    assert_eq!(writer.as_str()?, "#{\n    // First.\n    first: 1, // Keep.\n\n    last: 2\n}\n");
    let node = Node::parse_str(writer.as_str()?, Format::Rhai)?;
    assert_eq!(node.req::<i64>("first")?, 1);
    assert_eq!(node.req::<i64>("last")?, 2);
    Ok(())
}

#[test]
fn nested_end_comments_and_empty_container_tail_comments_parse() -> Result<(), NodeError> {
    let mut document = Document::map(|map| {
        map.entry_with("nested", |value| {
            value.map(|map| {
                map.entry("enabled", true)?;
                Ok(())
            })
        })?
        .end_comment("Nested preferences.");
        map.entry("empty", Vec::<i32>::new())?.tail_comment("Add entries here.")?;
        Ok(())
    })?;
    document.at_mut("nested")?.tail_comment("End of nested settings.")?;
    let output = render_rhai(&document)?;
    assert!(output.contains("}, // Nested preferences."));
    assert!(output.contains("// End of nested settings."));
    assert!(output.contains("empty: [\n        // Add entries here.\n    ]"));
    let node = Node::parse_str(&output, Format::Rhai)?;
    assert!(node.req::<bool>("nested.enabled")?);
    assert!(node.req::<Vec<i32>>("empty")?.is_empty());
    Ok(())
}

#[test]
fn expanded_sequence_keeps_undocumented_maps_compact() -> Result<(), NodeError> {
    let document = Document::seq(|seq| {
        seq.elem_document(
            Document::map(|map| {
                map.entry("name", "first")?;
                map.entry("enabled", true)?;
                Ok(())
            })?
            .compact(),
        )?;
        seq.elem_document(
            Document::map(|map| {
                map.entry("name", "second")?;
                map.entry("enabled", false)?;
                Ok(())
            })?
            .compact(),
        )?;
        Ok(())
    })?
    .expanded();
    let output = render_rhai(&document)?;
    assert!(output.contains("    #{ name: \"first\", enabled: true },\n"));
    assert!(output.contains("    #{ name: \"second\", enabled: false },\n"));
    let node = Node::parse_str(&output, Format::Rhai)?;
    assert_eq!(node.req::<String>("[1].name")?, "second");
    Ok(())
}

#[test]
fn leading_comments_expand_a_compact_container() -> Result<(), NodeError> {
    let document = Document::map(|map| {
        map.entry("enabled", true)?.comment("A setting.");
        Ok(())
    })?
    .compact();
    let output = render_rhai(&document)?;
    assert!(output.contains("#{\n    // A setting.\n    enabled: true"));
    assert!(Node::parse_str(&output, Format::Rhai)?.req::<bool>("enabled")?);
    Ok(())
}

#[test]
fn replacing_a_setting_retains_its_entry_comments() -> Result<(), NodeError> {
    let mut document = Document::map(|map| {
        map.entry("zoom_factor", 1.2)?
            .comment("Zoom multiplier.")
            .end_comment("Try 1.1 for finer steps.");
        Ok(())
    })?;
    document.replace("zoom_factor", 1.3)?;
    let output = render_rhai(&document)?;
    assert!(output.contains("// Zoom multiplier."));
    assert!(output.contains("zoom_factor: 1.3, // Try 1.1 for finer steps."));
    Ok(())
}

#[test]
fn nested_documents_retain_headers_and_container_footers() -> Result<(), NodeError> {
    let nested = Document::map(|map| {
        map.entry("enabled", true)?;
        Ok(())
    })?
    .header("Nested preferences.")
    .footer("End of this group.");
    let document = Document::map(|map| {
        map.entry_document("nested", nested)?;
        Ok(())
    })?;
    let output = render_rhai(&document)?;
    assert!(output.contains("    // Nested preferences.\n    nested: #{"));
    assert!(output.contains("        // End of this group.\n    }"));
    assert!(Node::parse_str(&output, Format::Rhai)?.req::<bool>("nested.enabled")?);

    let error = Document::map(|map| {
        map.entry_document("value", Document::from_value(&1)?.footer("Footer."))?;
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.path().map(ToString::to_string).as_deref(), Some("value"));
    Ok(())
}

#[test]
fn document_errors_retain_the_original_subtree_path() -> Result<(), NodeError> {
    let source = Node::parse_str("#{ outer: #{ value: 3 } }", Format::Rhai)?;
    let mut document = Document::from_node(source.req_node("outer")?.clone());
    document.replace("value", f64::NAN)?;
    let mut writer = RhaiWriter::to_string_writer();
    let error = writer.write(&document).unwrap_err();
    assert_eq!(error.path().map(ToString::to_string).as_deref(), Some("outer.value"));
    assert!(writer.as_str()?.is_empty());
    Ok(())
}

#[test]
fn comment_line_terminators_cannot_escape_the_comment() -> Result<(), NodeError> {
    let document = Document::map(|map| {
        map.entry("enabled", true)?.comment("One.\r\nTwo.\rThree.\nFour.\u{2028}Five.\u{2029}Six.");
        Ok(())
    })?
    .header("Header.")
    .footer("Footer.");
    let output = render_rhai(&document)?;
    for text in ["One.", "Two.", "Three.", "Four.", "Five.", "Six."] {
        assert!(output.contains(&format!("    // {text}\n")));
    }
    assert!(output.starts_with("// Header."));
    assert!(output.ends_with("// Footer.\n"));
    assert!(Node::parse_str(&output, Format::Rhai)?.req::<bool>("enabled")?);
    Ok(())
}

#[test]
fn multiline_end_comments_fail_before_writing() -> Result<(), NodeError> {
    for ending in ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
        let document = Document::map(|map| {
            map.entry("value", 1)?.end_comment(format!("First.{ending}Second."));
            Ok(())
        })?;
        let mut writer = RhaiWriter::to_string_writer();
        let error = writer.write(&document).unwrap_err();
        assert_eq!(error.path().map(ToString::to_string).as_deref(), Some("value"));
        assert!(writer.as_str()?.is_empty());
    }
    Ok(())
}

#[test]
fn reserved_keys_and_control_characters_round_trip() -> Result<(), NodeError> {
    let text = "quotes: \" and \\, controls: \0\u{001f}\n\t";
    let document = Document::map(|map| {
        map.entry("let", text)?;
        map.entry("key with spaces", "value")?;
        Ok(())
    })?;
    let output = render_rhai(&document)?;
    assert!(output.contains("\"let\":"));
    let node = Node::parse_str(&output, Format::Rhai)?;
    assert_eq!(node.req::<String>("let")?, text);
    assert_eq!(node.req::<String>("[\"key with spaces\"]")?, "value");
    Ok(())
}

#[test]
fn integral_floats_and_negative_zero_keep_their_float_syntax() -> Result<(), NodeError> {
    let document = Document::map(|map| {
        map.entry("whole", 1.0_f64)?;
        map.entry("negative_zero", -0.0_f64)?;
        Ok(())
    })?;
    let output = render_rhai(&document)?;
    assert!(output.contains("whole: 1.0"));
    assert!(output.contains("negative_zero: -0.0"));
    let node = Node::parse_str(&output, Format::Rhai)?;
    assert_eq!(node.req::<f64>("whole")?, 1.0);
    assert!(node.req::<f64>("negative_zero")?.is_sign_negative());
    Ok(())
}

#[test]
fn rhai_integer_boundaries_round_trip() -> Result<(), NodeError> {
    let document = Document::map(|map| {
        map.entry("minimum", i64::MIN)?;
        map.entry("maximum", i64::MAX)?;
        map.entry("unsigned_maximum", i64::MAX as u64)?;
        Ok(())
    })?;
    let output = render_rhai(&document)?;
    let node = Node::parse_str(&output, Format::Rhai)?;
    assert_eq!(node.req::<i64>("minimum")?, i64::MIN);
    assert_eq!(node.req::<i64>("maximum")?, i64::MAX);
    assert_eq!(node.req::<u64>("unsigned_maximum")?, i64::MAX as u64);
    Ok(())
}

#[test]
fn finite_float_extremes_and_subnormals_round_trip() -> Result<(), NodeError> {
    let doubles = [
        f64::MAX,
        f64::MIN,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        -f64::from_bits(1),
    ];
    let singles = [
        f32::MAX,
        f32::MIN,
        f32::MIN_POSITIVE,
        f32::from_bits(1),
        -f32::from_bits(1),
    ];
    let document = Document::map(|map| {
        map.entry("doubles", doubles)?;
        map.entry("singles", singles)?;
        Ok(())
    })?;
    let output = render_rhai(&document)?;
    let node = Node::parse_str(&output, Format::Rhai)?;
    assert_eq!(node.req::<Vec<f64>>("doubles")?, doubles);
    assert_eq!(node.req::<Vec<f32>>("singles")?, singles);
    Ok(())
}

#[test]
fn invalid_numbers_report_nested_paths_without_partial_output() -> Result<(), NodeError> {
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let document = Document::map(|map| {
            map.entry("values", vec![1.0, invalid])?;
            Ok(())
        })?;
        let mut writer = RhaiWriter::to_string_writer();
        let error = writer.write(&document).unwrap_err();
        assert_eq!(error.path().map(ToString::to_string).as_deref(), Some("values[1]"));
        assert!(writer.as_str()?.is_empty());
    }
    let document = Document::map(|map| {
        map.entry_with("limits", |value| {
            value.map(|map| {
                map.entry("maximum", u64::MAX)?;
                Ok(())
            })
        })?;
        Ok(())
    })?;
    let mut writer = RhaiWriter::to_string_writer();
    let error = writer.write(&document).unwrap_err();
    assert_eq!(error.path().map(ToString::to_string).as_deref(), Some("limits.maximum"));
    assert!(writer.as_str()?.is_empty());
    Ok(())
}

#[test]
fn rendering_does_not_visit_source_leaves() -> Result<(), NodeError> {
    let source = Node::parse_str("#{ nested: #{ value: 3 } }", Format::Rhai)?;
    let document = Document::from_node(source.clone());
    render_rhai(&document)?;
    assert!(!source.req_node("nested.value")?.as_leaf()?.is_visited());
    assert!(source.ensure_no_unknown_keys().is_err());
    Ok(())
}

#[test]
fn write_errors_retain_the_io_cause() -> Result<(), NodeError> {
    let document = Document::from_value(&42)?;
    let mut writer = RhaiWriter::new(FailingWriter);
    let error = writer.write(&document).unwrap_err();
    let mut cause: &(dyn Error + 'static) = &error;
    loop {
        if let Some(error) = cause.downcast_ref::<io::Error>() {
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            assert_eq!(error.to_string(), "writer stopped");
            break;
        }
        cause = cause.source().expect("The original I/O error remains in the source chain.");
    }
    Ok(())
}

#[test]
fn value_output_and_document_output_have_distinct_newline_contracts() -> Result<(), NodeError> {
    let mut value_writer = RhaiWriter::to_string_writer();
    value_writer.value(42)?;
    assert_eq!(value_writer.as_str()?, "42");
    assert_eq!(render_rhai(&Document::from_value(&42)?)?, "42\n");
    Ok(())
}

#[cfg(feature = "format-json5")]
#[test]
fn json5_renders_the_same_document_with_compatible_escapes() -> Result<(), NodeError> {
    use scry::writer::Json5Writer;

    let text = "controls: \0\u{001f}\n\t and \"quotes\"";
    let document = Document::map(|map| {
        map.entry("text", text)?.comment("Text settings.");
        map.entry("optional", Option::<String>::None)?.end_comment("Disabled.");
        map.entry("whole", 1.0_f64)?;
        Ok(())
    })?;
    let mut writer = Json5Writer::to_string_writer();
    writer.write(&document)?;
    let output = writer.into_string()?;
    assert!(output.contains("// Text settings."));
    assert!(output.contains("null, // Disabled."));
    assert!(!output.contains("\\u{"));
    let node = Node::parse_str(&output, Format::Json5)?;
    assert_eq!(node.req::<String>("text")?, text);
    assert_eq!(node.req::<Option<String>>("optional")?, None);
    assert_eq!(node.req::<f64>("whole")?, 1.0);
    Ok(())
}

#[cfg(feature = "format-json5")]
#[test]
fn json5_rejects_integers_unsupported_by_its_reader_before_output() -> Result<(), NodeError> {
    use scry::writer::Json5Writer;

    let document = Document::map(|map| {
        map.entry("maximum", u64::MAX)?;
        Ok(())
    })?;
    let mut writer = Json5Writer::to_string_writer();
    let error = writer.write(&document).unwrap_err();
    assert_eq!(error.path().map(ToString::to_string).as_deref(), Some("maximum"));
    assert!(writer.as_str()?.is_empty());
    Ok(())
}

fn render_rhai(document: &Document) -> Result<String, NodeError> {
    let mut writer = RhaiWriter::to_string_writer();
    writer.write(document)?;
    writer.into_string()
}

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "writer stopped"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
