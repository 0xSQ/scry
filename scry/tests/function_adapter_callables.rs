//! Checks nested callable hooks, independent capabilities, and ordinary function coercions.

use std::net::IpAddr;
use std::num::Wrapping;
use std::sync::atomic::{AtomicUsize, Ordering};

use scry::convert::{read, write};
use scry::desc::DescKind;
use scry::kit::key_values::{self, KeyValues};
use scry::node::Format;
use scry::{Config, Desc, Describe, FromNode, KeyPath, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[derive(FromNode)]
struct Input(
    #[scry(from_node_with(|node| read::vec(node, |node| read::option(node, read_wrapped))))]
    Vec<Option<Wrapping<u16>>>,
);

#[derive(ToNode)]
struct Output(
    #[scry(to_node_with(|values| write::list(values, |value| write::option(value, |values| write::list(values, write_wrapped)))))]
     Vec<Option<Vec<Wrapping<OutputOnly>>>>,
);

#[derive(Describe)]
#[allow(dead_code)]
struct Description(
    #[scry(describe_with(|| Desc::list(Desc::plain("address string").nullable())))]
    Vec<Option<IpAddr>>,
);

#[derive(Config)]
struct Settings {
    #[scry(from_node_with(|node| read::option(node, |node| read::vec(node, read_wrapped))),
           describe_with(|| Desc::list(Desc::plain("wrapped integer")).nullable()))]
    values: Option<Vec<Wrapping<u16>>>,
}

#[derive(FromNode, ToNode, Describe)]
struct AddressBook {
    #[scry(from_node_with(|node| key_values::read_with(node, |node| read::vec(node, read_address))),
           to_node_with(|values| key_values::write_with(values, |values| write::list(values, write_address))),
           describe_with(|| key_values::description(Desc::list(Desc::plain("IP address")))))]
    groups: KeyValues<Vec<IpAddr>>,
}

#[derive(FromNode)]
struct RunningTotal(
    #[scry(from_node_with(|node| {
        let mut total = 0u16;
        read::vec(node, |node| {
            total += node.as_type::<u16>()?;
            Ok(Wrapping(total))
        })
    }))]
    Vec<Wrapping<u16>>,
);

#[derive(FromNode, ToNode, Describe)]
struct FactoryField {
    #[scry(
        default = 19,
        from_node_with(make_reader()),
        to_node_with(make_writer()),
        describe_with(make_describer())
    )]
    value: u16,
}

#[derive(ToNode)]
struct Coercions {
    #[scry(to_node_with(write_text))]
    direct: String,
    #[scry(to_node_with((write_text)))]
    grouped: String,
    #[scry(with(text_module))]
    bundled: String,
    #[scry(to_node_with(write_slice))]
    numbers: Vec<u16>,
}

#[derive(Describe)]
#[allow(dead_code)]
struct HookDefault {
    #[scry(default = 7, describe_with(|| Desc::plain("level")))]
    value: u16,
}

// ---------------------------------------------------------------------------------------------- //

#[test]
fn nested_callbacks_require_only_the_requested_capability() {
    let input: Input = parse("[17, (), 23]");
    assert_eq!(input.0, [Some(Wrapping(17)), None, Some(Wrapping(23))]);

    let output = Output(vec![
        Some(vec![Wrapping(OutputOnly(29)), Wrapping(OutputOnly(31))]),
        None,
        Some(Vec::new()),
    ]);
    assert_eq!(
        output.to_node().unwrap().as_type::<Vec<Option<Vec<u16>>>>().unwrap(),
        [Some(vec![29, 31]), None, Some(Vec::new())]
    );
    assert_eq!(output.0[0].as_ref().unwrap()[1].0 .0, 31);

    let DescKind::List { item } = Description::describe().kind else {
        panic!("expected a list description");
    };
    assert!(item.nullable);
    assert_eq!(item.type_label(), "address string | null");
}

#[test]
fn config_composes_input_and_description_without_output() {
    let missing: Settings = parse("#{}");
    assert!(missing.values.is_none());
    let present: Settings = parse("#{ values: [2, 5] }");
    assert_eq!(present.values, Some(vec![Wrapping(2), Wrapping(5)]));

    let DescKind::Struct { fields } = Settings::describe().kind else {
        panic!("expected a struct description");
    };
    assert!(fields[0].optional);
    assert!(fields[0].value.nullable);
    let DescKind::List { item } = &fields[0].value.kind else {
        panic!("expected a nullable list description");
    };
    assert_eq!(item.type_label(), "wrapped integer");
}

#[test]
fn complete_field_types_infer_nested_kit_reader_and_borrowed_writer_closures() {
    let source = Node::parse_str(
        r#"#{ groups: [["local", ["127.0.0.1", "::1"]], ["empty", []]] }"#,
        Format::Rhai,
    )
    .unwrap();
    let value: AddressBook = source.as_type().unwrap();
    assert_eq!(value.groups.0[0].0, "local");
    assert_eq!(value.groups.0[0].1.len(), 2);
    assert!(value.groups.0[1].1.is_empty());
    source.ensure_no_unknown_keys().unwrap();
    let output = value.to_node().unwrap();
    assert_eq!(output.as_type::<AddressBook>().unwrap().groups.0, value.groups.0);
    assert!(AddressBook::describe().validate_path("groups").is_ok());

    let source = Node::parse_str(
        r#"#{ scope: #{ groups: [["local", ["127.0.0.1", "invalid"]]] } }"#,
        Format::Rhai,
    )
    .unwrap();
    let error = source.req_node("scope").unwrap().as_type::<AddressBook>().err().unwrap();
    assert_eq!(
        error.path(),
        Some(&KeyPath::from_keys(["scope", "groups"]).push_index(0).push_index(1).push_index(1))
    );
}

#[test]
fn a_child_callback_can_borrow_mutable_state_for_the_entire_traversal() {
    let values: RunningTotal = parse("[2, 3, 7]");
    assert_eq!(values.0, [Wrapping(2), Wrapping(5), Wrapping(12)]);
}

#[test]
fn callable_factories_are_evaluated_once_per_requested_operation_and_skip_missing_input() {
    READER_FACTORIES.store(0, Ordering::SeqCst);
    WRITER_FACTORIES.store(0, Ordering::SeqCst);
    DESCRIBER_FACTORIES.store(0, Ordering::SeqCst);
    let missing: FactoryField = parse("#{}");
    assert_eq!(missing.value, 19);
    assert_eq!(READER_FACTORIES.load(Ordering::SeqCst), 0);

    let present: FactoryField = parse("#{ value: () }");
    assert_eq!(present.value, 7);
    assert_eq!(READER_FACTORIES.load(Ordering::SeqCst), 1);
    assert_eq!(present.to_node().unwrap().req::<u16>("value").unwrap(), 7);
    assert_eq!(WRITER_FACTORIES.load(Ordering::SeqCst), 1);
    let DescKind::Struct { fields } = FactoryField::describe().kind else {
        panic!("expected fields")
    };
    assert_eq!(fields[0].value.type_label(), "factory value");
    assert_eq!(DESCRIBER_FACTORIES.load(Ordering::SeqCst), 1);
}

#[test]
fn function_paths_and_modules_preserve_string_and_slice_coercions() {
    let value = Coercions {
        direct: "direct".to_owned(),
        grouped: "grouped".to_owned(),
        bundled: "bundled".to_owned(),
        numbers: vec![2, 5],
    };
    let output = value.to_node().unwrap();
    assert_eq!(output.req::<String>("direct").unwrap(), "direct");
    assert_eq!(output.req::<String>("grouped").unwrap(), "grouped");
    assert_eq!(output.req::<String>("bundled").unwrap(), "bundled");
    assert_eq!(output.req::<Vec<u16>>("numbers").unwrap(), [2, 5]);
}

#[test]
fn individual_description_hooks_retain_existing_literal_default_metadata() {
    let DescKind::Struct { fields } = HookDefault::describe().kind else {
        panic!("expected fields")
    };
    assert!(fields[0].optional);
    assert_eq!(fields[0].default_display.as_deref(), Some("7"));
    assert_eq!(fields[0].value.type_label(), "level");
}

// ---------------------------------------------------------------------------------------------- //

fn read_wrapped(node: &Node) -> Result<Wrapping<u16>, NodeError> {
    Ok(Wrapping(node.as_type()?))
}

fn write_wrapped(value: &Wrapping<OutputOnly>) -> Result<Node, NodeError> {
    value.0.to_node()
}

struct OutputOnly(u16);
impl ToNode for OutputOnly {
    fn to_node(&self) -> Result<Node, NodeError> {
        self.0.to_node()
    }
}

fn read_address(node: &Node) -> Result<IpAddr, NodeError> {
    node.as_type::<String>()?.parse().map_err(|error| {
        NodeError::invalid_value_with_source(&node.path, "invalid IP address", error)
    })
}

fn write_address(value: &IpAddr) -> Result<Node, NodeError> {
    value.to_string().to_node()
}

static READER_FACTORIES: AtomicUsize = AtomicUsize::new(0);
static WRITER_FACTORIES: AtomicUsize = AtomicUsize::new(0);
static DESCRIBER_FACTORIES: AtomicUsize = AtomicUsize::new(0);

fn make_reader() -> impl FnOnce(&Node) -> Result<u16, NodeError> {
    READER_FACTORIES.fetch_add(1, Ordering::SeqCst);
    let token = vec![7u16];
    move |node| {
        Ok(node.as_type::<Option<u16>>()?.unwrap_or_else(|| token.into_iter().next().unwrap()))
    }
}

fn make_writer() -> impl FnOnce(&u16) -> Result<Node, NodeError> {
    WRITER_FACTORIES.fetch_add(1, Ordering::SeqCst);
    let token = Box::new(());
    move |value| {
        drop(token);
        value.to_node()
    }
}

fn make_describer() -> impl FnOnce() -> Desc {
    DESCRIBER_FACTORIES.fetch_add(1, Ordering::SeqCst);
    let label = "factory value".to_owned();
    move || Desc::plain(label)
}

fn write_text(value: &str) -> Result<Node, NodeError> {
    value.to_node()
}
fn write_slice(value: &[u16]) -> Result<Node, NodeError> {
    write::list(value, ToNode::to_node)
}

mod text_module {
    pub fn to_node(value: &str) -> Result<super::Node, super::NodeError> {
        super::write_text(value)
    }
}

fn parse<T: FromNode>(source: &str) -> T {
    Node::parse_str(source, Format::Rhai).unwrap().as_type().unwrap()
}
