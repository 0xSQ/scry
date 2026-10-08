//! Checks generic function adapters and generated identifier hygiene.

use std::marker::PhantomData;
use std::num::Wrapping;

use scry::convert::{read, write};
use scry::desc::DescKind;
use scry::node::Format;
use scry::{Config, Desc, Describe, FromNode, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

#[derive(Config, ToNode)]
struct GenericValues<T = u16, const N: usize = 2>
where
    T: PartialEq + FromNode + ToNode + Describe,
{
    #[scry(from_node_with(locked::from_node), to_node_with(locked::to_node),
           describe_with(locked::describe::<[T; N]>))]
    values: std::sync::Mutex<[T; N]>,
    #[scry(default = N)]
    count: usize,
}

#[derive(FromNode)]
struct ReadMutex<T: FromNode> {
    #[scry(from_node_with(selection::from_node))]
    value: std::sync::Mutex<T>,
}

#[derive(ToNode)]
struct WriteMutex<T: ToNode> {
    #[scry(to_node_with(selection::to_node))]
    value: std::sync::Mutex<T>,
}

#[derive(Describe)]
#[allow(dead_code)]
struct DescribeMutex<T: Describe> {
    #[scry(describe_with(selection::describe::<T>))]
    value: std::sync::Mutex<T>,
}

#[derive(FromNode, ToNode, Describe)]
struct TypeDependency<P: Encoding> {
    #[scry(from_node_with(locked::from_node), to_node_with(locked::to_node),
           describe_with(locked::describe::<P::Value>))]
    value: std::sync::Mutex<P::Value>,
    #[scry(default = PhantomData, with(marker))]
    marker: PhantomData<P>,
}

#[derive(FromNode, ToNode, Describe)]
struct ConstAdapter<const N: usize>
where
    [(); N]: OffsetAmount,
{
    #[scry(from_node_with(offset::from_node::<N>), to_node_with(offset::to_node::<N>),
           describe_with(offset::describe::<N>))]
    value: Wrapping<u16>,
}

#[derive(FromNode, ToNode, Describe)]
struct Value<T: Encoding> {
    #[scry(from_node_with(locked::from_node), to_node_with(locked::to_node),
           describe_with(locked::describe::<T::Value>))]
    plain: std::sync::Mutex<T::Value>,
    #[scry(from_node_with(locked::from_node), to_node_with(locked::to_node),
           describe_with(locked::describe::<<T as Encoding>::Value>))]
    qualified: std::sync::Mutex<<T as Encoding>::Value>,
}

#[derive(ToNode, Describe)]
struct Borrowed<'a, T: ToNode + Describe + ?Sized> {
    #[scry(to_node_with(ToNode::to_node), describe_with(T::describe))]
    value: &'a T,
}

#[derive(ToNode, Describe)]
struct Tail<T: AsRef<[u16]> + ?Sized> {
    #[scry(to_node_with(|value| write::list(value.as_ref(), ToNode::to_node)),
           describe_with(<[u16]>::describe))]
    value: T,
}

#[derive(Debug, PartialEq, FromNode, ToNode)]
struct Tree<T> {
    value: T,
    #[scry(default = Vec::new(),
           from_node_with(|node| read::vec(node, <self::Tree<T> as FromNode>::from_node)),
           to_node_with(|values| write::list(values, ToNode::to_node)))]
    children: Vec<self::Tree<T>>,
}

#[derive(Debug, PartialEq, FromNode, ToNode)]
enum Branch<T> {
    Leaf(T),
    Children(
        #[scry(from_node_with(|node| read::vec(node, <Self as FromNode>::from_node)),
               to_node_with(|values| write::list(values, ToNode::to_node)))]
        Vec<Self>,
    ),
}

#[derive(FromNode, ToNode, Describe)]
struct Hygiene<Policy, Result, Kind, Option>
where
    Result: FromNode + ToNode + Describe,
    Kind: FromNode + ToNode + Describe,
    Option: FromNode + ToNode + Describe,
{
    #[scry(from_node_with(locked::from_node), to_node_with(locked::to_node),
           describe_with(locked::describe::<Result>))]
    value: std::sync::Mutex<Result>,
    #[scry(from_node_with(Node::as_type::<Kind>), to_node_with(ToNode::to_node),
           describe_with(Kind::describe))]
    kind: Kind,
    #[scry(from_node_with(Node::as_type::<Option>), to_node_with(ToNode::to_node),
           describe_with(Option::describe))]
    option: Option,
    #[scry(default = PhantomData, with(marker))]
    policy: PhantomData<Policy>,
}

// ---------------------------------------------------------------------------------------------- //

#[test]
fn foreign_generic_targets_preserve_parameter_defaults_and_user_constraints() {
    let value: GenericValues = parse("#{ values: [3, 5] }");
    assert_eq!(*value.values.lock().unwrap(), [3, 5]);
    assert_eq!(value.count, 2);
    let output = value.to_node().unwrap();
    assert_eq!(output.req::<[u16; 2]>("values").unwrap(), [3, 5]);
    assert!(GenericValues::<u16, 2>::describe().validate_path("values[1]").is_ok());

    let value: GenericValues<u8, 3> = parse("#{ values: [2, 4, 6] }");
    assert_eq!(*value.values.lock().unwrap(), [2, 4, 6]);
    assert_eq!(value.count, 3);
    assert_eq!(value.to_node().unwrap().req::<[u8; 3]>("values").unwrap(), [2, 4, 6]);
    assert!(GenericValues::<u8, 3>::describe().validate_path("values[2]").is_ok());
}

#[test]
fn each_generic_operation_requires_only_its_selected_capability() {
    let input: ReadMutex<InputOnly> = parse("#{ value: 17 }");
    assert_eq!(input.value.lock().unwrap().0, 17);

    let output = WriteMutex {
        value: std::sync::Mutex::new(OutputOnly(23)),
    }
    .to_node()
    .unwrap();
    assert_eq!(output.req::<u16>("value").unwrap(), 23);

    let DescKind::Struct { fields } = DescribeMutex::<ShapeOnly>::describe().kind else {
        panic!("expected fields");
    };
    assert_eq!(fields[0].value.type_label(), "description only");
}

#[test]
fn type_only_hook_dependencies_preserve_explicit_user_requirements() {
    let value: TypeDependency<Codec> = parse("#{ value: 29 }");
    assert_eq!(*value.value.lock().unwrap(), 29);
    let output = value.to_node().unwrap();
    assert_eq!(output.req::<u16>("value").unwrap(), 29);
    assert_eq!(output.req::<Option<u16>>("marker").unwrap(), None);
    assert!(TypeDependency::<Codec>::describe().validate_path("value").is_ok());
}

#[test]
fn const_hook_dependencies_preserve_explicit_conditional_requirements() {
    let value: ConstAdapter<2> = parse("#{ value: 7 }");
    assert_eq!(value.value, Wrapping(9));
    assert_eq!(value.to_node().unwrap().req::<u16>("value").unwrap(), 7);
    let DescKind::Struct { fields } = ConstAdapter::<2>::describe().kind else {
        panic!("expected fields");
    };
    assert_eq!(fields[0].value.type_label(), "integer offset by 2");

    let value: ConstAdapter<3> = parse("#{ value: 7 }");
    assert_eq!(value.value, Wrapping(10));
    assert_eq!(value.to_node().unwrap().req::<u16>("value").unwrap(), 7);
}

#[test]
fn associated_and_qualified_targets_keep_the_callers_meaning() {
    let value: Value<Codec> = parse("#{ plain: 5, qualified: 8 }");
    assert_eq!(*value.plain.lock().unwrap(), 5);
    assert_eq!(*value.qualified.lock().unwrap(), 8);
    let output = value.to_node().unwrap();
    assert_eq!(output.req::<u16>("plain").unwrap(), 5);
    assert_eq!(output.req::<u16>("qualified").unwrap(), 8);
    for name in ["plain", "qualified"] {
        assert!(Value::<Codec>::describe().validate_path(name).is_ok());
    }
}

#[test]
fn borrowed_lifetimes_and_unsized_output_and_descriptions_remain_supported() {
    let text = "borrowed text".to_owned();
    let value = Borrowed {
        value: text.as_str(),
    };
    assert_eq!(value.to_node().unwrap().req::<String>("value").unwrap(), text);
    assert!(Borrowed::<str>::describe().validate_path("value").is_ok());

    let sized = Tail { value: [2u16, 7] };
    let view: &Tail<[u16]> = &sized;
    assert_eq!(view.to_node().unwrap().req::<Vec<u16>>("value").unwrap(), [2, 7]);
    assert!(Tail::<[u16]>::describe().validate_path("value[0]").is_ok());
}

#[test]
fn recursive_hook_targets_do_not_create_circular_impl_predicates() {
    let value: Tree<u16> = parse("#{ value: 1, children: [#{ value: 2 }] }");
    assert_eq!(value.children[0].value, 2);
    assert!(value.children[0].children.is_empty());
    assert_eq!(value.to_node().unwrap().as_type::<Tree<u16>>().unwrap(), value);

    let value: Branch<u16> = parse("#{ children: [#{ leaf: 3 }, #{ children: [#{ leaf: 5 }] }] }");
    assert_eq!(
        value,
        Branch::Children(vec![Branch::Leaf(3), Branch::Children(vec![Branch::Leaf(5)])])
    );
    assert_eq!(value.to_node().unwrap().as_type::<Branch<u16>>().unwrap(), value);
}

#[test]
fn generic_and_runtime_like_identifiers_preserve_the_callers_meaning() {
    let value: Hygiene<Codec, u16, u8, bool> = parse("#{ value: 13, kind: 4, option: true }");
    assert_eq!(*value.value.lock().unwrap(), 13);
    assert_eq!(value.kind, 4);
    assert!(value.option);
    let output = value.to_node().unwrap();
    assert_eq!(output.req::<u16>("value").unwrap(), 13);
    assert!(output.req::<bool>("option").unwrap());
    assert!(Hygiene::<Codec, u16, u8, bool>::describe().validate_path("option").is_ok());

    let input = Node::parse_str("#{ value: 13, kind: 4 }", Format::Rhai).unwrap();
    let error = match input.as_type::<Hygiene<Codec, u16, u8, bool>>() {
        Ok(_) => panic!("a type parameter named Option is still required"),
        Err(error) => error,
    };
    assert_eq!(error.path().unwrap().to_string(), "option");
}

// ---------------------------------------------------------------------------------------------- //

mod locked {
    use super::*;

    pub fn from_node<T: FromNode>(node: &Node) -> Result<std::sync::Mutex<T>, NodeError> {
        Ok(std::sync::Mutex::new(node.as_type()?))
    }

    pub fn to_node<T: ToNode>(value: &std::sync::Mutex<T>) -> Result<Node, NodeError> {
        value.lock().unwrap().to_node()
    }

    pub fn describe<T: Describe>() -> Desc {
        T::describe()
    }
}

mod selection {
    pub use super::locked::{describe, from_node, to_node};
}

mod marker {
    use super::*;

    pub fn from_node<T>(node: &Node) -> Result<PhantomData<T>, NodeError> {
        match node.as_type::<Option<u16>>()? {
            None => Ok(PhantomData),
            Some(_) => Err(NodeError::invalid_value(&node.path, "marker must be null")),
        }
    }

    pub fn to_node<T>(_value: &PhantomData<T>) -> Result<Node, NodeError> {
        ().to_node()
    }

    pub fn describe() -> Desc {
        <()>::describe()
    }
}

mod offset {
    use super::*;

    pub fn from_node<const N: usize>(node: &Node) -> Result<Wrapping<u16>, NodeError>
    where
        [(); N]: OffsetAmount,
    {
        Ok(Wrapping(node.as_type::<u16>()? + <[(); N]>::AMOUNT))
    }

    pub fn to_node<const N: usize>(value: &Wrapping<u16>) -> Result<Node, NodeError>
    where
        [(); N]: OffsetAmount,
    {
        (value.0 - <[(); N]>::AMOUNT).to_node()
    }

    pub fn describe<const N: usize>() -> Desc
    where
        [(); N]: OffsetAmount,
    {
        Desc::plain(format!("integer offset by {}", <[(); N]>::AMOUNT))
    }
}

trait OffsetAmount {
    const AMOUNT: u16;
}
impl OffsetAmount for [(); 2] {
    const AMOUNT: u16 = 2;
}
impl OffsetAmount for [(); 3] {
    const AMOUNT: u16 = 3;
}

trait Encoding {
    type Value: FromNode + ToNode + Describe;
}
struct Codec;
impl Encoding for Codec {
    type Value = u16;
}

struct InputOnly(u16);
impl FromNode for InputOnly {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        Ok(Self(node.as_type()?))
    }
}

struct OutputOnly(u16);
impl ToNode for OutputOnly {
    fn to_node(&self) -> Result<Node, NodeError> {
        self.0.to_node()
    }
}

struct ShapeOnly;
impl Describe for ShapeOnly {
    fn describe() -> Desc {
        Desc::plain("description only")
    }
}

fn parse<T: FromNode>(source: &str) -> T {
    Node::parse_str(source, Format::Rhai).unwrap().as_type().unwrap()
}
