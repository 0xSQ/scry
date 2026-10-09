//! Covers complete-type descriptions, nullability, and their CLI consumers.
#![allow(dead_code)]

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use clap::Command;
use scry::cli::setup::{ExposeMap, QueryArgs};
use scry::desc::{DescKind, EntryRef, FieldDesc};
use scry::node::Format;
use scry::{Config, Desc, Describe, Node, ToNode};

// ---------------------------------------------------------------------------------------------- //

type Matrix = Vec<Vec<u32>>;
type NullableNumber = Option<u32>;

#[derive(Describe)]
struct Shapes {
    direct: Vec<Vec<u32>>,
    alias: Matrix,
    array: [Vec<Option<u32>>; 2],
    tuple: (Option<Matrix>, Vec<Option<u32>>),
}

#[derive(Debug, Config, ToNode)]
struct NullPolicies {
    samples: Vec<Option<u32>>,
    threshold: Option<u32>,
    alias: NullableNumber,
    #[scry(default = 0)]
    retries: u32,
}

#[derive(Describe)]
struct Child {
    count: u32,
}

#[derive(Describe)]
struct Wrappers {
    shared: Arc<Vec<Option<Child>>>,
    owned: Box<Matrix>,
    local: Rc<Option<Child>>,
    borrowed: &'static [Option<u32>],
    text: &'static str,
    mutable: &'static mut [u32],
    path: &'static Path,
    raw: Node,
}

#[derive(Describe)]
struct TransparentMatrix(Matrix);

#[derive(Describe)]
struct Pair(Matrix, Option<Child>);

#[derive(Describe)]
enum Payloads {
    Unit,
    Matrix(Matrix),
    Pair(Matrix, Option<Child>),
    Named {
        matrix: Matrix,
        child: Option<Child>,
    },
}

#[derive(Debug, PartialEq, Config)]
enum Mode {
    #[scry(default)]
    Fast,
    Careful,
}

#[derive(Describe)]
struct EnumContexts {
    required: Mode,
    optional: Option<Mode>,
    #[scry(from_defaults)]
    recursive: Mode,
    items: Vec<Mode>,
}

struct Custom;

#[derive(Describe)]
struct Hooked {
    /// Samples in a custom encoding.
    #[scry(rename = "encoded.samples", describe_with(custom_desc), default = None)]
    custom: Option<Vec<Custom>>,
}

fn custom_desc() -> Desc {
    Desc::plain("encoded samples").nullable()
}

#[test]
fn named_fields_compose_their_complete_types_and_aliases_identically() {
    let desc = Shapes::describe();
    assert_eq!(field(&desc, "direct").value.type_label(), "list[list[u32]]");
    assert_eq!(field(&desc, "alias").value.type_label(), "list[list[u32]]");
    assert_eq!(field(&desc, "array").value.type_label(), "list[list[u32]]");

    for path in [
        "direct[0][1]",
        "alias[0][1]",
        "array[0][1]",
        "tuple[0][1][2]",
        "tuple[1][2]",
    ] {
        desc.validate_path(path).unwrap();
        assert!(desc.entry_at_path(path).is_some(), "missing entry at {path}");
    }
    assert!(desc.validate_path("direct[0][1][2]").is_err());
    assert!(desc.validate_path("tuple[2]").is_err());
}

#[test]
fn nullability_and_omission_describe_different_policies() {
    let desc = NullPolicies::describe();
    let samples = field(&desc, "samples");
    assert!(!samples.optional);
    assert!(!samples.value.nullable);
    let DescKind::List { item } = &samples.value.kind else {
        panic!("expected list items");
    };
    assert!(item.nullable);
    assert!(field(&desc, "threshold").optional);
    assert!(field(&desc, "threshold").value.nullable);
    assert!(!field(&desc, "alias").optional);
    assert!(field(&desc, "alias").value.nullable);
    assert!(field(&desc, "retries").optional);
    assert!(!field(&desc, "retries").value.nullable);
    assert_eq!(field(&desc, "retries").default_display.as_deref(), Some("0"));

    let input = Node::parse_str("#{ samples: [1, ()], alias: () }", Format::Rhai).unwrap();
    let value = input.as_type::<NullPolicies>().unwrap();
    assert_eq!(value.samples, [Some(1), None]);
    assert_eq!(value.threshold, None);
    assert_eq!(value.alias, None);
    assert_eq!(value.retries, 0);
    assert_eq!(value.to_node().unwrap().req::<NullableNumber>("alias").unwrap(), None);
    let input = Node::parse_str("#{ samples: [], retries: () , alias: () }", Format::Rhai).unwrap();
    assert!(input.as_type::<NullPolicies>().is_err());
    let input = Node::parse_str("#{ samples: [] }", Format::Rhai).unwrap();
    assert!(input.as_type::<NullPolicies>().is_err());

    let rendered = desc.display();
    assert!(rendered.contains("◆ samples: list[u32]"));
    assert!(rendered.contains("◇ threshold: u32"));
    assert!(rendered.contains("◆ alias: u32"));
    assert!(rendered.contains("◇ retries: u32 → 0"));
}

#[test]
fn owning_and_borrowed_wrappers_forward_shape_and_nullability() {
    let desc = Wrappers::describe();
    assert_eq!(field(&desc, "owned").value.type_label(), Matrix::describe().type_label());
    assert_eq!(field(&desc, "borrowed").value.type_label(), "list[u32]");
    assert_eq!(field(&desc, "text").value.type_label(), "string");
    assert_eq!(field(&desc, "mutable").value.type_label(), "list[u32]");
    assert_eq!(field(&desc, "path").value.type_label(), "path");
    assert_eq!(field(&desc, "raw").value.type_label(), "value");
    for path in ["shared[2].count", "owned[0][1]", "local.count"] {
        desc.validate_path(path).unwrap();
        assert!(desc.entry_at_path(path).is_some());
    }
    assert!(!field(&desc, "local").optional);
    assert!(field(&desc, "local").value.nullable);
}

#[test]
fn positional_descriptions_use_the_same_complete_type_composition() {
    assert_eq!(TransparentMatrix::describe().type_label(), "list[list[u32]]");
    TransparentMatrix::describe().validate_path("[0][1]").unwrap();
    Pair::describe().validate_path("[0][1][2]").unwrap();
    Pair::describe().validate_path("[1].count").unwrap();

    let desc = Payloads::describe();
    for path in [
        "matrix[0][1]",
        "pair[0][1][2]",
        "pair[1].count",
        "named.matrix[0][1]",
        "named.child.count",
    ] {
        desc.validate_path(path).unwrap();
        assert!(desc.entry_at_path(path).is_some(), "missing entry at {path}");
    }
    assert_eq!(
        desc.entry_at_path("named.child").unwrap().display().lines().next(),
        Some("◇ child")
    );
}

#[test]
fn nullable_structures_and_enums_keep_children_and_contextual_defaults() {
    let child = Option::<Child>::describe();
    assert!(child.nullable);
    assert!(!child.is_leaf());
    assert_eq!(child.display(), "◆ count: u32\n");
    child.validate_path("count").unwrap();
    assert!(child.entry_at_path("count").is_some());

    let desc = EnumContexts::describe();
    assert!(!has_default(&field(&desc, "required").value));
    assert!(!has_default(&field(&desc, "optional").value));
    assert!(has_default(&field(&desc, "recursive").value));
    let DescKind::List { item } = &field(&desc, "items").value.kind else {
        panic!("expected list of enums");
    };
    assert!(has_default(item));
    let optional = &field(&desc, "optional").value;
    assert!(optional.nullable);
    assert_eq!(optional.unit_enum_variants().unwrap().len(), 2);
    optional.validate_path("fast").unwrap();
}

#[test]
fn repeated_options_share_one_nullable_shape() {
    let desc = Option::<Option<u32>>::describe();
    assert!(desc.nullable);
    assert_eq!(desc.type_label(), "u32");
    assert_eq!(desc.display(), "u32\n");
    assert!(desc.is_leaf());
    assert!(desc.validate_path("child").is_err());
}

#[test]
fn custom_descriptions_replace_the_complete_value_without_native_bounds() {
    let desc = Hooked::describe();
    let custom = field(&desc, r#"["encoded.samples"]"#);
    assert_eq!(custom.value.type_label(), "encoded samples");
    assert!(custom.optional);
    assert_eq!(custom.doc, "Samples in a custom encoding.");
    assert!(desc.display().contains("encoded.samples: encoded samples"));
}

#[test]
fn cli_description_queries_accept_deep_lists_and_keep_useful_item_hints() {
    let query = QueryArgs::new().desc("desc", None);
    for (path, expected) in [("direct[0][1]", "◆ 1: u32"), ("array[0][1]", "◆ 1: u32")] {
        let matches = query
            .augment(Command::new("test"))
            .try_get_matches_from(["test", "--desc", path])
            .unwrap();
        let output = query.check_desc::<Shapes>(&matches).unwrap().unwrap();
        assert_eq!(output.trim(), expected);
    }
}

#[test]
fn cli_description_queries_show_optional_struct_fields_and_enum_choices() {
    let query = QueryArgs::new().desc("desc", None);
    let command = query.augment(Command::new("test"));
    let matches = command.clone().try_get_matches_from(["test", "--desc"]).unwrap();
    assert_eq!(query.check_desc::<Option<Child>>(&matches).unwrap().unwrap(), "◆ count: u32\n");
    assert_eq!(query.check_desc::<Option<Mode>>(&matches).unwrap().unwrap(), "» fast\n› careful\n");

    let matches = command.clone().try_get_matches_from(["test", "--desc", "named.child"]).unwrap();
    assert_eq!(
        query.check_desc::<Payloads>(&matches).unwrap().unwrap(),
        "◇ child\n   ◆ count: u32\n"
    );

    let matches = command.try_get_matches_from(["test", "--desc", "optional"]).unwrap();
    assert_eq!(
        query.check_desc::<EnumContexts>(&matches).unwrap().unwrap(),
        "◇ optional\n   › fast\n   › careful\n"
    );
}

#[test]
fn nullable_unit_enums_retain_cli_possible_values() {
    let desc = EnumContexts::describe();
    let mut expose = ExposeMap::new();
    expose.option("optional");
    let command = expose.augment(Command::new("test"), &desc);
    let arg = command.get_arguments().find(|arg| arg.get_id() == "optional").unwrap();
    let values: Vec<_> = arg
        .get_value_parser()
        .possible_values()
        .unwrap()
        .map(|value| value.get_name().to_owned())
        .collect();
    assert_eq!(values, ["fast", "careful"]);
    command.clone().try_get_matches_from(["test", "--optional", "careful"]).unwrap();
    assert!(command.try_get_matches_from(["test", "--optional", "unknown"]).is_err());
}

fn field<'a>(desc: &'a Desc, path: &str) -> &'a FieldDesc {
    let Some(EntryRef::Field(field)) = desc.entry_at_path(path) else {
        panic!("expected field at {path}");
    };
    field
}

fn has_default(desc: &Desc) -> bool {
    desc.unit_enum_variants().unwrap().iter().any(|variant| variant.is_default())
}
