//! Checks readable template metadata without changing the generated configuration values.

use std::path::PathBuf;

use scry::desc::{DefaultDesc, FieldDesc};
use scry::node::Format;
use scry::writer::{
    format_template, format_template_with_config, TemplateConfig, TemplateMode, TemplateSpacing,
    TemplateTypeHints,
};
use scry::{Config, Desc, Describe, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[derive(Config)]
#[allow(dead_code)]
struct Settings {
    /// Names the source image.
    input: PathBuf,
    /// Controls the detection threshold.
    #[scry(default = 4)]
    threshold: u8,
    /// Enables an additional thumbnail export.
    thumbnail: Option<Thumbnail>,
    #[scry(default = false)]
    verbose: bool,
}

#[derive(Config)]
#[allow(dead_code)]
struct Thumbnail {
    /// Names the thumbnail destination.
    path: PathBuf,
    /// Limits the longest side in pixels.
    #[scry(default = 256)]
    size: u32,
}

#[test]
fn field_metadata_precedes_defaults_and_typed_holes() -> Result<(), NodeError> {
    for format in formats() {
        let text = format_template::<Settings>(format.clone(), TemplateMode::Ref)?;
        assert_line(&text, "// Names the source image. Type: path. Required.");
        assert_entry(&text, &format, "input", "<path>,", true);
        assert_line(&text, "// Controls the detection threshold. Type: u8. Optional.");
        assert_entry(&text, &format, "threshold", "4,", true);
        assert_line(&text, "// Type: bool. Optional.");
        assert_entry(&text, &format, "verbose", "false,", true);
        assert!(!text.contains("// type:"), "{text}");
        assert!(!text.contains("Type: object"), "{text}");
        let node = Node::parse_str(&text, format)?;
        assert!(node.as_map()?.is_empty());
    }
    Ok(())
}

#[test]
fn disabled_nested_objects_keep_one_comment_marker_at_each_natural_indentation(
) -> Result<(), NodeError> {
    for format in formats() {
        let text = format_template::<Settings>(format.clone(), TemplateMode::Ref)?;
        assert_line(
            &text,
            "// Enables an additional thumbnail export. Optional. Omitted by default.",
        );
        assert_line(&text, "// Names the thumbnail destination. Type: path. Required.");
        assert_entry(&text, &format, "path", "<path>,", true);
        assert_line(&text, "// Limits the longest side in pixels. Type: u32. Optional.");
        assert_entry(&text, &format, "size", "256,", true);
        let path_entry = format!("// {}: <path>,", entry_key(&format, "path"));
        let path_line = text.lines().find(|line| line.trim_start() == path_entry).unwrap();
        assert!(path_line.starts_with(&format!("        {path_entry}")), "{text}");
        for line in text.lines() {
            if let Some(comment) = line.trim_start().strip_prefix("//") {
                assert!(!comment.trim_start().starts_with("//"), "{text}");
            }
        }
        Node::parse_str(&text, format)?;
    }
    Ok(())
}

#[test]
fn multiline_docs_append_type_and_policy_to_the_last_nonempty_line() -> Result<(), NodeError> {
    struct MultilineDocs;
    impl Describe for MultilineDocs {
        fn describe() -> Desc {
            Desc::structure(vec![
                FieldDesc::new("value", Desc::plain("string"))
                    .with_doc("First paragraph.\r\n\r\nFinal line.\u{2028}"),
                FieldDesc::new("custom", Desc::plain("duration expression"))
                    .with_doc("Uses a custom expression."),
            ])
        }
    }
    for format in formats() {
        let text = format_template::<MultilineDocs>(format.clone(), TemplateMode::Ref)?;
        assert_line(&text, "// First paragraph.");
        assert_line(&text, "// Final line. Type: string. Required.");
        assert_line(&text, "// Uses a custom expression. Type: duration expression. Required.");
        assert_entry(&text, &format, "custom", "<value>,", true);
        assert!(!text.contains('\r') && !text.contains('\u{2028}'), "{text}");
        Node::parse_str(&text, format)?;
    }
    Ok(())
}

#[test]
fn actual_nested_defaults_receive_metadata_without_being_replaced_by_child_defaults(
) -> Result<(), NodeError> {
    struct NestedDefaults;
    impl Describe for NestedDefaults {
        fn describe() -> Desc {
            let service = Desc::structure(vec![
                FieldDesc::new("name", Desc::plain("string")).with_doc("Names the service."),
                FieldDesc::new("port", Desc::plain("u16"))
                    .with_doc("Selects the service port.")
                    .with_fallback(
                        DefaultDesc::unknown().with_value(|| 8080u16.to_node().map(Some)),
                    ),
            ]);
            Desc::structure(vec![
                FieldDesc::new("connection", service.clone())
                    .with_doc("Configures the primary service.")
                    .with_fallback(DefaultDesc::unknown().with_value(|| {
                        Node::parse_str("#{ name: \"primary\", port: 9090 }", Format::Rhai)
                            .map(Some)
                    })),
                FieldDesc::new("replicas", Desc::list(service))
                    .with_doc("Lists the replica services.")
                    .with_fallback(DefaultDesc::unknown().with_value(|| {
                        Node::parse_str("[#{ name: \"replica\", port: 9191 }]", Format::Rhai)
                            .map(Some)
                    })),
            ])
        }
    }
    for format in formats() {
        for mode in [TemplateMode::Ref, TemplateMode::Explicit] {
            let text = format_template::<NestedDefaults>(format.clone(), mode)?;
            assert_line(&text, "// Configures the primary service. Optional.");
            assert_line(&text, "// Lists the replica services. Optional.");
            assert_eq!(
                text.matches("Names the service. Type: string. Required.").count(),
                2,
                "{text}"
            );
            assert_eq!(
                text.matches("Selects the service port. Type: u16. Optional.").count(),
                2,
                "{text}"
            );
            assert_entry(&text, &format, "port", "9090,", mode == TemplateMode::Ref);
            assert_entry(&text, &format, "port", "9191,", mode == TemplateMode::Ref);
            assert!(!text.contains("8080"), "{text}");
            let node = Node::parse_str(&text, format.clone())?;
            if mode == TemplateMode::Explicit {
                assert_eq!(node.req::<u16>("connection.port")?, 9090);
                assert_eq!(node.req::<u16>("replicas[0].port")?, 9191);
            } else {
                assert!(node.as_map()?.is_empty());
            }
        }
    }
    Ok(())
}

#[test]
fn all_presets_keep_their_omission_and_default_semantics() -> Result<(), NodeError> {
    for format in formats() {
        for mode in [TemplateMode::Min, TemplateMode::Ref, TemplateMode::Explicit] {
            let text = format_template::<Settings>(format.clone(), mode)?;
            let mut node = Node::parse_str(&text, format.clone())?;
            assert!(node.opt_node("input")?.is_none());
            if mode == TemplateMode::Explicit {
                assert_eq!(node.req::<u8>("threshold")?, 4);
                assert!(!node.req::<bool>("verbose")?);
                assert!(node.req::<Option<Thumbnail>>("thumbnail")?.is_none());
            } else {
                assert!(node.as_map()?.is_empty());
            }
            node.set_value("input", "source.png")?;
            let settings = node.as_type::<Settings>()?;
            assert_eq!(settings.input, PathBuf::from("source.png"));
            assert_eq!(settings.threshold, 4);
            assert!(!settings.verbose);
            assert!(settings.thumbnail.is_none());
        }
    }
    Ok(())
}

#[test]
fn selecting_a_container_and_one_child_keeps_sibling_entries_disabled() -> Result<(), NodeError> {
    for format in formats() {
        let template = format_template::<Settings>(format.clone(), TemplateMode::Ref)?;
        let opening = format!("// {}:", entry_key(&format, "thumbnail"));
        let child = format!("// {}:", entry_key(&format, "path"));
        let edited = template
            .lines()
            .map(|line| {
                let content = line.trim_start();
                let is_opening = content.starts_with(&opening);
                let is_child = content.starts_with(&child);
                let is_closing = line.starts_with("    // },") && !line.starts_with("        ");
                if is_opening || is_child || is_closing {
                    let indentation = &line[..line.len() - content.len()];
                    format!("{indentation}{}", content.strip_prefix("// ").unwrap())
                        .replace("<path>", "\"thumbnail.png\"")
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let node = Node::parse_str(&edited, format)?;
        let thumbnail = node.req::<Thumbnail>("thumbnail")?;
        assert_eq!(thumbnail.path, PathBuf::from("thumbnail.png"));
        assert_eq!(thumbnail.size, 256);
        assert!(node.opt_node("thumbnail.size")?.is_none(), "{edited}");
        assert!(node.opt_node("threshold")?.is_none(), "{edited}");
    }
    Ok(())
}

#[test]
fn multiline_type_hints_stay_on_one_line_and_url_values_keep_their_slashes() -> Result<(), NodeError>
{
    struct HintsAndUrls;
    impl Describe for HintsAndUrls {
        fn describe() -> Desc {
            Desc::structure(vec![
                FieldDesc::new("duration", Desc::plain("duration\r\nexpression\u{2028} seconds")),
                FieldDesc::new("url", Desc::plain("string")).with_fallback(
                    DefaultDesc::unknown()
                        .with_value(|| "https://example.org/a//b".to_node().map(Some)),
                ),
            ])
        }
    }
    for format in formats() {
        for mode in [TemplateMode::Ref, TemplateMode::Explicit] {
            let text = format_template::<HintsAndUrls>(format.clone(), mode)?;
            assert_line(&text, "// Type: duration expression seconds. Required.");
            assert_entry(&text, &format, "duration", "<value>,", true);
            assert_line(&text, "// Type: string. Optional.");
            assert_entry(
                &text,
                &format,
                "url",
                "\"https://example.org/a//b\",",
                mode == TemplateMode::Ref,
            );
            let node = Node::parse_str(&text, format.clone())?;
            if mode == TemplateMode::Explicit {
                assert_eq!(node.req::<String>("url")?, "https://example.org/a//b");
            }
        }
    }
    Ok(())
}

#[test]
fn list_and_tuple_placeholders_can_be_enabled_without_repairing_punctuation(
) -> Result<(), NodeError> {
    struct Containers;
    impl Describe for Containers {
        fn describe() -> Desc {
            Desc::structure(vec![
                FieldDesc::new("list", Desc::list(Desc::plain("u8"))),
                FieldDesc::new(
                    "tuple",
                    Desc::tuple(vec![Desc::plain("u8"), Desc::plain("string")]),
                ),
            ])
        }
    }
    for format in formats() {
        let text = format_template::<Containers>(format.clone(), TemplateMode::Ref)?;
        assert_line(&text, "// Type: list<u8>. Required.");
        assert_entry(&text, &format, "list", "[", true);
        assert_line(&text, "// <u8>,");
        assert_line(&text, "// <string>,");
        let edited = text
            .lines()
            .map(|line| {
                let content = line.trim_start();
                if content.starts_with(&format!("// {}:", entry_key(&format, "list")))
                    || content.starts_with(&format!("// {}:", entry_key(&format, "tuple")))
                    || content == "// <u8>,"
                    || content == "// <string>,"
                    || content == "// ],"
                {
                    let indentation = &line[..line.len() - content.len()];
                    format!("{indentation}{}", content.strip_prefix("// ").unwrap())
                        .replace("<u8>", "7")
                        .replace("<string>", "\"seven\"")
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let node = Node::parse_str(&edited, format)?;
        assert_eq!(node.req::<Vec<u8>>("list")?, [7]);
        assert_eq!(node.req::<(u8, String)>("tuple")?, (7, "seven".to_owned()));
    }
    Ok(())
}

#[test]
fn depth_limited_holes_keep_their_comma_outside_explanatory_comments() -> Result<(), NodeError> {
    struct Deep;
    impl Describe for Deep {
        fn describe() -> Desc {
            let mut description = Desc::plain("string");
            for _ in 0..12 {
                description = Desc::structure(vec![FieldDesc::new("child", description)]);
            }
            description
        }
    }
    for format in formats() {
        let text = format_template::<Deep>(format.clone(), TemplateMode::Min)?;
        assert!(text.contains("Shape expansion stopped at the depth limit."), "{text}");
        assert_entry(&text, &format, "child", "<value>,", true);
        Node::parse_str(&text, format.clone())?;
        let edited = text
            .lines()
            .map(|line| {
                if line.contains("<value>") {
                    let content = line.trim_start();
                    let indentation = &line[..line.len() - content.len()];
                    format!("{indentation}{}", content.strip_prefix("// ").unwrap())
                        .replace("<value>", "\"example\"")
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        Node::parse_str(&edited, format)?;
    }
    Ok(())
}

#[test]
fn real_scalar_collections_stay_compact_with_one_field_type_annotation() -> Result<(), NodeError> {
    #[derive(Config)]
    #[allow(dead_code)]
    struct CompactDefaults {
        #[scry(default = vec![1, 2])]
        values: Vec<u8>,
        #[scry(default = (7, "seven".to_owned()))]
        pair: (u8, String),
    }
    for format in formats() {
        for mode in [TemplateMode::Ref, TemplateMode::Explicit] {
            let text = format_template::<CompactDefaults>(format.clone(), mode)?;
            assert_line(&text, "// Type: list<u8>. Optional.");
            assert_entry(&text, &format, "values", "[1, 2],", mode == TemplateMode::Ref);
            assert_entry(&text, &format, "pair", "[7, \"seven\"],", mode == TemplateMode::Ref);
            let node = Node::parse_str(&text, format.clone())?;
            if mode == TemplateMode::Explicit {
                assert_eq!(node.req::<Vec<u8>>("values")?, [1, 2]);
                assert_eq!(node.req::<(u8, String)>("pair")?, (7, "seven".to_owned()));
            } else {
                assert!(node.as_map()?.is_empty());
            }
        }
    }
    Ok(())
}

#[test]
fn formatting_options_cover_shapes_and_nested_defaults_without_changing_values(
) -> Result<(), NodeError> {
    struct Presentation;
    impl Describe for Presentation {
        fn describe() -> Desc {
            let service = Desc::structure(vec![
                FieldDesc::new("port", Desc::plain("u16")).with_doc("Selects the port."),
                FieldDesc::new("codes", Desc::list(Desc::plain("u8")))
                    .with_doc("Selects the codes."),
            ]);
            Desc::structure(vec![
                FieldDesc::new("name", Desc::plain("string")).with_doc("Names the output."),
                FieldDesc::new("scale", Desc::plain("f64"))
                    .with_doc("Scales the output.")
                    .with_fallback(
                        DefaultDesc::unknown().with_value(|| 1.5f64.to_node().map(Some)),
                    ),
                FieldDesc::new("service", service.clone()).with_fallback(
                    DefaultDesc::unknown().with_value(|| {
                        Node::parse_str("#{ port: 9090, codes: [1, 2] }", Format::Rhai).map(Some)
                    }),
                ),
                FieldDesc::new("replicas", Desc::list(service.clone())).with_fallback(
                    DefaultDesc::unknown().with_value(|| {
                        Node::parse_str("[#{ port: 9191, codes: [3, 4] }]", Format::Rhai).map(Some)
                    }),
                ),
                FieldDesc::new("preview", service).with_fallback(
                    DefaultDesc::unknown().with_value(|| Option::<u8>::None.to_node().map(Some)),
                ),
            ])
        }
    }

    for format in formats() {
        for mode in [TemplateMode::Min, TemplateMode::Ref, TemplateMode::Explicit] {
            let baseline = format_template::<Presentation>(format.clone(), mode)?;
            let baseline =
                Node::parse_str(&baseline, format.clone())?.to_string_as(Format::Rhai)?;
            for spacing in [TemplateSpacing::Blank, TemplateSpacing::Compact] {
                for type_hints in [
                    TemplateTypeHints::Above,
                    TemplateTypeHints::Inline,
                    TemplateTypeHints::None,
                ] {
                    let config = TemplateConfig {
                        mode,
                        spacing,
                        type_hints,
                    };
                    let text =
                        format_template_with_config::<Presentation>(format.clone(), &config)?;
                    let node = Node::parse_str(&text, format.clone())?;
                    assert_eq!(node.to_string_as(Format::Rhai)?, baseline, "{config:?}\n{text}");
                    assert_entry(&text, &format, "name", "<string>,", true);
                    assert_line(
                        &text,
                        if type_hints == TemplateTypeHints::Above {
                            "// Names the output. Type: string. Required."
                        } else {
                            "// Names the output. Required."
                        },
                    );
                    if type_hints != TemplateTypeHints::Above {
                        assert!(!text.contains("Type:"), "{text}");
                    }
                    if type_hints != TemplateTypeHints::Inline {
                        assert!(!text.contains("// type:"), "{text}");
                    }
                    let lines: Vec<_> = text.lines().collect();
                    for (index, line) in lines.iter().enumerate() {
                        if line.contains("Scales the output.")
                            || line.contains("Selects the codes.")
                        {
                            assert_eq!(
                                lines[index - 1].is_empty(),
                                spacing == TemplateSpacing::Blank,
                                "{config:?}\n{text}",
                            );
                        }
                    }
                    if spacing == TemplateSpacing::Compact {
                        assert!(!text.contains("\n\n"), "{text}");
                    }
                    if mode != TemplateMode::Min {
                        let suffix = if type_hints == TemplateTypeHints::Inline {
                            " // type: list<u8>"
                        } else {
                            ""
                        };
                        assert_entry(
                            &text,
                            &format,
                            "codes",
                            &format!("[1, 2],{suffix}"),
                            mode == TemplateMode::Ref,
                        );
                        assert_entry(
                            &text,
                            &format,
                            "codes",
                            &format!("[3, 4],{suffix}"),
                            mode == TemplateMode::Ref,
                        );
                        let occurrences = if mode == TemplateMode::Ref { 3 } else { 2 };
                        assert_eq!(
                            text.matches("list<u8>").count(),
                            if type_hints == TemplateTypeHints::None {
                                0
                            } else {
                                occurrences
                            },
                            "{text}",
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

fn formats() -> Vec<Format> {
    vec![
        Format::Rhai,
        #[cfg(feature = "format-json5")]
        Format::Json5,
    ]
}

fn entry_key(format: &Format, key: &str) -> String {
    if *format == Format::Rhai {
        key.to_owned()
    } else {
        format!("\"{key}\"")
    }
}

fn assert_entry(text: &str, format: &Format, key: &str, value: &str, commented: bool) {
    let prefix = if commented { "// " } else { "" };
    assert_line(text, &format!("{prefix}{}: {value}", entry_key(format, key)));
}

fn assert_line(text: &str, expected: &str) {
    assert!(text.lines().any(|line| line.trim_start() == expected), "Missing {expected}\n{text}");
}
