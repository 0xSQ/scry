//! Checks composed policies without borrowing unrelated capabilities from their targets.

use std::marker::PhantomData;
use std::net::IpAddr;
use std::num::Wrapping;
use std::sync::Mutex;

use scry::desc::DescKind;
use scry::node::Format;
use scry::via::Native;
use scry::{
    Config, Desc, Describe, DescribeVia, FromNode, FromNodeVia, Node, NodeError, ToNode, ToNodeVia,
};

// ---------------------------------------------------------------------------------------------- //

#[derive(FromNode)]
struct Input(#[scry(via(Vec<Option<Read>>))] Vec<Option<Wrapping<u16>>>);

#[derive(ToNode)]
struct Output(#[scry(via(Vec<Option<Vec<Write>>>))] Vec<Option<Vec<Wrapping<OutputOnly>>>>);

#[derive(Describe)]
#[allow(dead_code)]
struct Description(#[scry(via(Vec<Option<Shape>>))] Vec<Option<IpAddr>>);

#[derive(Config)]
struct Settings {
    #[scry(via(Option<Vec<ReadAndDescribe>>))]
    values: Option<Vec<Wrapping<u16>>>,
}

#[derive(FromNode, ToNode, Describe)]
struct LockedValues<T = u16>
where
    T: PartialEq,
{
    #[scry(via(policies::NullableLocks<T>))]
    values: Vec<Option<Mutex<T>>>,
}

#[derive(Debug, PartialEq, FromNode, ToNode)]
struct Tree<T> {
    value: T,
    #[scry(default = Vec::new(), via(policies::Branches<Native>))]
    children: Vec<self::Tree<T>>,
}

#[derive(Debug, PartialEq, FromNode, ToNode)]
enum Branch<T> {
    Leaf(T),
    Children(#[scry(via(Vec<Native>))] Vec<Self>),
}

struct Read;

impl FromNodeVia<Wrapping<u16>> for Read {
    fn from_node(node: &Node) -> Result<Wrapping<u16>, NodeError> {
        Ok(Wrapping(node.as_type()?))
    }
}

struct Write;

impl ToNodeVia<Wrapping<OutputOnly>> for Write {
    fn to_node(value: &Wrapping<OutputOnly>) -> Result<Node, NodeError> {
        value.0.to_node()
    }
}

struct Shape;

impl DescribeVia<IpAddr> for Shape {
    fn describe() -> Desc {
        Desc::plain("address string")
    }
}

struct ReadAndDescribe;

impl FromNodeVia<Wrapping<u16>> for ReadAndDescribe {
    fn from_node(node: &Node) -> Result<Wrapping<u16>, NodeError> {
        <Read as FromNodeVia<Wrapping<u16>>>::from_node(node)
    }
}

impl DescribeVia<Wrapping<u16>> for ReadAndDescribe {
    fn describe() -> Desc {
        Desc::plain("wrapped integer")
    }
}

struct Locked<T>(PhantomData<T>);

impl<T: FromNode> FromNodeVia<Mutex<T>> for Locked<T> {
    fn from_node(node: &Node) -> Result<Mutex<T>, NodeError> {
        Ok(Mutex::new(node.as_type()?))
    }
}

impl<T: ToNode> ToNodeVia<Mutex<T>> for Locked<T> {
    fn to_node(value: &Mutex<T>) -> Result<Node, NodeError> {
        value.lock().unwrap().to_node()
    }
}

impl<T: Describe> DescribeVia<Mutex<T>> for Locked<T> {
    fn describe() -> Desc {
        T::describe()
    }
}

#[derive(PartialEq)]
struct InputOnly(u16);

impl FromNode for InputOnly {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        Ok(Self(node.as_type()?))
    }
}

#[derive(PartialEq)]
struct OutputOnly(u16);

impl ToNode for OutputOnly {
    fn to_node(&self) -> Result<Node, NodeError> {
        self.0.to_node()
    }
}

#[derive(PartialEq)]
struct ShapeOnly;

impl Describe for ShapeOnly {
    fn describe() -> Desc {
        Desc::plain("description only")
    }
}

mod policies {
    // ------------------------------------------------------------------------------------------ //

    pub type NullableLocks<T> = Vec<Option<super::Locked<T>>>;
    pub type Branches<A> = Vec<A>;
}

// ---------------------------------------------------------------------------------------------- //

#[test]
fn composed_policies_require_only_the_requested_inner_capability() {
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
fn config_accepts_read_and_describe_composition_without_output() {
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
fn foreign_generic_targets_preserve_defaults_constraints_and_selected_capabilities() {
    let defaults: LockedValues = parse("#{ values: [7, ()] }");
    assert_eq!(*defaults.values[0].as_ref().unwrap().lock().unwrap(), 7);
    assert!(defaults.values[1].is_none());

    let input: LockedValues<InputOnly> = parse("#{ values: [11, ()] }");
    assert_eq!(input.values[0].as_ref().unwrap().lock().unwrap().0, 11);
    assert!(input.values[1].is_none());

    let output = LockedValues {
        values: vec![Some(Mutex::new(OutputOnly(13))), None],
    };
    assert_eq!(
        output.to_node().unwrap().req::<Vec<Option<u16>>>("values").unwrap(),
        [Some(13), None]
    );
    assert_eq!(output.values[0].as_ref().unwrap().lock().unwrap().0, 13);

    let DescKind::Struct { fields } = LockedValues::<ShapeOnly>::describe().kind else {
        panic!("expected a struct description");
    };
    let DescKind::List { item } = &fields[0].value.kind else {
        panic!("expected a list description");
    };
    assert!(item.nullable);
    assert_eq!(item.type_label(), "description only | null");
}

#[test]
fn native_delegation_keeps_capabilities_independent_and_supports_unsized_targets() {
    let node = Node::parse_str("19", Format::Rhai).unwrap();
    assert_eq!(<Native as FromNodeVia<InputOnly>>::from_node(&node).unwrap().0, 19);
    assert_eq!(
        <Native as ToNodeVia<OutputOnly>>::to_node(&OutputOnly(23))
            .unwrap()
            .as_type::<u16>()
            .unwrap(),
        23
    );
    assert_eq!(<Native as DescribeVia<ShapeOnly>>::describe().type_label(), "description only");

    assert_eq!(
        <Native as ToNodeVia<str>>::to_node("borrowed text").unwrap().as_type::<String>().unwrap(),
        "borrowed text"
    );
    assert_eq!(<Native as DescribeVia<str>>::describe().type_label(), "string");
}

#[test]
fn native_container_recursion_avoids_circular_derived_requirements() {
    let tree: Tree<u16> = parse("#{ value: 1, children: [#{ value: 2 }] }");
    assert_eq!(tree.children[0].value, 2);
    assert!(tree.children[0].children.is_empty());
    assert_eq!(tree.to_node().unwrap().as_type::<Tree<u16>>().unwrap(), tree);

    let branch: Branch<u16> = parse("#{ children: [#{ leaf: 3 }, #{ children: [#{ leaf: 5 }] }] }");
    assert_eq!(
        branch,
        Branch::Children(vec![Branch::Leaf(3), Branch::Children(vec![Branch::Leaf(5)])])
    );
    assert_eq!(branch.to_node().unwrap().as_type::<Branch<u16>>().unwrap(), branch);
}

// ---------------------------------------------------------------------------------------------- //

fn parse<T: FromNode>(source: &str) -> T {
    Node::parse_str(source, Format::Rhai).unwrap().as_type().unwrap()
}
