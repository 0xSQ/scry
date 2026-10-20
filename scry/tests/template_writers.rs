//! Checks automatically generated configuration scaffolds across presets and languages.

use std::sync::atomic::{AtomicUsize, Ordering};

use scry::desc::{DefaultDesc, FieldDesc};
use scry::node::Format;
use scry::writer::{format_template, TemplateMode};
use scry::{Config, Desc, Describe, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

/// Configures a small batch command without supplying a separate starter value.
#[derive(Debug, PartialEq, Config)]
struct Settings {
    /// Names this batch.
    name: String,
    /// Connects to the processing service.
    connection: Connection,
    /// Lists the jobs to process.
    jobs: Vec<Job>,
    /// Limits retry attempts.
    #[scry(default = 3)]
    attempts: u32,
    /// Labels generated output.
    #[scry(default = "neutral".to_owned())]
    label: String,
    /// Adds optional labels.
    #[scry(default = Vec::new())]
    tags: Vec<String>,
    /// Enables an optional notification destination.
    notification: Option<Notification>,
    /// Limits concurrent work when specified.
    #[scry(default = Some(9))]
    budget: Option<u32>,
    /// Holds recursively defaulted settings.
    #[scry(from_defaults)]
    logging: Logging,
    /// Uses a literal key containing punctuation.
    #[scry(rename = "literal.key", default = false)]
    flag: bool,
}

#[derive(Debug, PartialEq, Config)]
struct Connection {
    /// Selects the service port.
    #[scry(default = 8080)]
    port: u16,
}

#[derive(Debug, PartialEq, Config)]
struct Job {
    /// Names the work item.
    input: String,
    /// Repeats the work item.
    #[scry(default = 1)]
    repeat: u32,
}

#[derive(Debug, PartialEq, Config)]
struct Notification {
    /// Names the destination.
    destination: String,
}

#[derive(Debug, PartialEq, Config)]
struct Logging {
    /// Prints progress details.
    #[scry(default = false)]
    verbose: bool,
}

#[test]
fn presets_keep_required_holes_and_decode_equivalently_after_filling_them() -> Result<(), NodeError>
{
    for format in formats() {
        for mode in [TemplateMode::Min, TemplateMode::Ref, TemplateMode::Explicit] {
            let text = format_template::<Settings>(format.clone(), mode)?;
            let mut node = Node::parse_str(&text, format.clone())?;

            assert!(node.opt_node("name")?.is_none(), "{format}: {mode:?}\n{text}");
            assert!(node.opt_node("jobs")?.is_none(), "{format}: {mode:?}\n{text}");
            assert!(node.req_node("connection")?.kind.is_map());
            assert!(node.as_type::<Settings>().is_err());

            node.set_value("name", "example-batch")?;
            node.set_node("jobs", Node::parse_str("[]", Format::Rhai)?)?;
            assert_eq!(
                node.as_type::<Settings>()?,
                Settings {
                    name: "example-batch".to_owned(),
                    connection: Connection { port: 8080 },
                    jobs: Vec::new(),
                    attempts: 3,
                    label: "neutral".to_owned(),
                    tags: Vec::new(),
                    notification: None,
                    budget: Some(9),
                    logging: Logging { verbose: false },
                    flag: false,
                },
                "{format}: {mode:?}\n{text}",
            );
            node.ensure_no_unknown_keys()?;
        }
    }
    Ok(())
}

#[test]
fn minimal_keeps_required_empty_objects_and_omits_defaulted_sections() -> Result<(), NodeError> {
    assert_eq!(TemplateMode::default(), TemplateMode::Min);
    for format in formats() {
        let text = format_template::<Settings>(format.clone(), TemplateMode::Min)?;
        let node = Node::parse_str(&text, format)?;
        assert!(node.req_node("connection")?.as_map()?.is_empty());
        for key in [
            "attempts",
            "label",
            "tags",
            "notification",
            "budget",
            "logging",
        ] {
            assert!(node.opt_node(key)?.is_none(), "Unexpected active {key}.\n{text}");
        }
        assert!(!text.contains("Limits retry attempts."));
        assert!(text.contains("Names this batch."));
    }
    Ok(())
}

#[test]
fn reference_documents_defaults_and_optional_shapes_without_enabling_them() -> Result<(), NodeError>
{
    for format in formats() {
        let text = format_template::<Settings>(format.clone(), TemplateMode::Ref)?;
        let node = Node::parse_str(&text, format)?;
        for key in [
            "attempts",
            "label",
            "tags",
            "notification",
            "budget",
            "logging",
        ] {
            assert!(node.opt_node(key)?.is_none(), "Unexpected active {key}.\n{text}");
        }
        for explanation in [
            "Limits retry attempts.",
            "Labels generated output.",
            "Names the destination.",
            "Names the work item.",
        ] {
            assert!(text.contains(explanation), "Missing {explanation}\n{text}");
        }
        assert!(text.contains("neutral"));
    }
    Ok(())
}

#[test]
fn explicit_writes_defaults_but_does_not_enable_features_or_invent_list_items(
) -> Result<(), NodeError> {
    for format in formats() {
        let text = format_template::<Settings>(format.clone(), TemplateMode::Explicit)?;
        let node = Node::parse_str(&text, format)?;
        assert_eq!(node.req::<u32>("attempts")?, 3);
        assert_eq!(node.req::<String>("label")?, "neutral");
        assert_eq!(node.req::<Option<u32>>("budget")?, Some(9));
        assert_eq!(node.req::<Option<Notification>>("notification")?, None);
        assert!(node.req::<Vec<String>>("tags")?.is_empty());
        assert!(!node.req::<bool>("logging.verbose")?);
        assert!(!node.req::<bool>("[\"literal.key\"]")?);
        assert!(node.opt_node("literal")?.is_none());
        assert!(node.opt_node("jobs")?.is_none());
    }
    Ok(())
}

#[test]
fn default_expressions_run_only_when_the_selected_preset_needs_the_value() -> Result<(), NodeError>
{
    static CALLS: AtomicUsize = AtomicUsize::new(0);

    fn counted_default() -> u32 {
        CALLS.fetch_add(1, Ordering::SeqCst);
        17
    }

    #[derive(Config)]
    struct LazyDefault {
        #[scry(default = counted_default())]
        #[allow(dead_code)]
        count: u32,
    }

    let _description = LazyDefault::describe();
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    format_template::<LazyDefault>(Format::Rhai, TemplateMode::Min)?;
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    format_template::<LazyDefault>(Format::Rhai, TemplateMode::Ref)?;
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    let text = format_template::<LazyDefault>(Format::Rhai, TemplateMode::Explicit)?;
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    assert_eq!(Node::parse_str(&text, Format::Rhai)?.req::<u32>("count")?, 17);
    Ok(())
}

#[test]
fn partial_aggregates_do_not_evaluate_children_before_rendering_their_scaffold(
) -> Result<(), NodeError> {
    static CALLS: AtomicUsize = AtomicUsize::new(0);

    fn counted_default() -> u32 {
        CALLS.fetch_add(1, Ordering::SeqCst);
        17
    }

    #[derive(Config)]
    #[allow(dead_code)]
    struct Partial {
        #[scry(default = counted_default())]
        count: u32,
        required: String,
    }

    #[derive(Config)]
    #[allow(dead_code)]
    struct Root {
        #[scry(from_defaults)]
        partial: Partial,
    }

    for format in formats() {
        for mode in [TemplateMode::Min, TemplateMode::Ref, TemplateMode::Explicit] {
            CALLS.store(0, Ordering::SeqCst);
            let description = Root::describe();
            let _help = description.display();
            assert_eq!(CALLS.load(Ordering::SeqCst), 0);
            let text = format_template::<Root>(format.clone(), mode)?;
            assert_eq!(CALLS.load(Ordering::SeqCst), usize::from(mode != TemplateMode::Min));
            if mode != TemplateMode::Min {
                assert!(text.contains("17"));
                assert!(text.contains("Default value unavailable"));
            }
            Node::parse_str(&text, format.clone())?;
        }
    }
    Ok(())
}

#[test]
fn standalone_enum_descriptions_expose_the_same_wire_choices() -> Result<(), NodeError> {
    #[derive(Describe)]
    #[allow(dead_code)]
    enum Choice {
        First,
        Second,
    }

    #[derive(Describe)]
    #[allow(dead_code)]
    struct Root {
        choice: Choice,
    }

    for format in formats() {
        let text = format_template::<Root>(format.clone(), TemplateMode::Ref)?;
        assert!(text.contains("Choices: \"first\", \"second\"."));
        assert!(text.contains("<string>"));
        Node::parse_str(&text, format)?;
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

#[test]
fn parent_defaults_keep_their_actual_values_instead_of_child_defaults() -> Result<(), NodeError> {
    struct Settings;
    impl Describe for Settings {
        fn describe() -> Desc {
            let child = FieldDesc::new("port", Desc::plain("u16"))
                .with_fallback(DefaultDesc::unknown().with_value(|| 8080u16.to_node().map(Some)));
            Desc::structure(vec![FieldDesc::new("connection", Desc::structure(vec![child]))
                .with_fallback(
                    DefaultDesc::unknown()
                        .with_value(|| Node::parse_str("#{ port: 9090 }", Format::Rhai).map(Some)),
                )])
        }
    }
    for format in formats() {
        let minimal = format_template::<Settings>(format.clone(), TemplateMode::Min)?;
        assert!(Node::parse_str(&minimal, format.clone())?.opt_node("connection")?.is_none());
        let reference = format_template::<Settings>(format.clone(), TemplateMode::Ref)?;
        assert!(reference.contains("9090"));
        assert!(!reference.contains("8080"));
        let explicit = format_template::<Settings>(format.clone(), TemplateMode::Explicit)?;
        assert_eq!(Node::parse_str(&explicit, format)?.req::<u16>("connection.port")?, 9090);
    }
    Ok(())
}

#[test]
fn unavailable_defaults_remain_commented_without_inventing_a_value() -> Result<(), NodeError> {
    #[derive(Config)]
    #[allow(dead_code)]
    struct Custom(String);
    #[derive(Config)]
    #[allow(dead_code)]
    struct Settings {
        #[scry(default = Custom("actual custom value".to_owned()))]
        custom: Custom,
    }
    for format in formats() {
        let text = format_template::<Settings>(format.clone(), TemplateMode::Explicit)?;
        assert!(text.contains("Default value unavailable"));
        assert!(Node::parse_str(&text, format)?.opt_node("custom")?.is_none());
    }
    Ok(())
}

#[test]
fn bounded_shape_expansion_keeps_deep_required_objects_syntactically_valid() -> Result<(), NodeError>
{
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
        for mode in [TemplateMode::Min, TemplateMode::Ref, TemplateMode::Explicit] {
            let text = format_template::<Deep>(format.clone(), mode)?;
            assert!(text.contains("depth limit"));
            Node::parse_str(&text, format.clone())?;
        }
    }
    Ok(())
}
