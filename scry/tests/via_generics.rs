//! Checks independent Via requirements, generic policies, and generated identifier hygiene.

use std::marker::PhantomData;
use std::num::Wrapping;

use scry::desc::DescKind;
use scry::node::Format;
use scry::{
    Config, Desc, Describe, DescribeVia, FromNode, FromNodeVia, Node, NodeError, ToNode, ToNodeVia,
};
use unsized_target::Tail;

// ---------------------------------------------------------------------------------------------- //

#[derive(Config, ToNode)]
struct GenericValues<T = u16, const N: usize = 2>
where
    T: PartialEq,
{
    #[scry(via(Locked))]
    values: std::sync::Mutex<[T; N]>,
    #[scry(default = N)]
    count: usize,
}

#[derive(FromNode, ToNode, Describe)]
struct Mutex<T> {
    #[scry(via(selection::LockPolicy))]
    value: std::sync::Mutex<T>,
}

#[derive(FromNode, ToNode, Describe)]
struct PolicyDependency<P> {
    #[scry(via(P))]
    value: std::sync::Mutex<u16>,
    #[scry(default = PhantomData, via(Marker))]
    marker: PhantomData<P>,
}

#[derive(FromNode, ToNode, Describe)]
struct ConstPolicy<const N: usize> {
    #[scry(via(Offset<N>))]
    value: Wrapping<u16>,
}

#[derive(FromNode, ToNode, Describe)]
struct Value<T: Encoding> {
    #[scry(via(Locked))]
    plain: T::Value,
    #[scry(via(Locked))]
    qualified: <T as Encoding>::Value,
}

#[derive(ToNode, Describe)]
struct Borrowed<'a, T: ?Sized> {
    #[scry(via(Delegate))]
    value: &'a T,
}

// Input narrows T to Sized, while output and description also support an unsized T.
#[allow(clippy::needless_maybe_sized)]
mod unsized_target {
    use scry::{Describe, FromNode, ToNode};

    use super::ArrayPolicy;

    // ------------------------------------------------------------------------------------------ //

    #[derive(FromNode, ToNode, Describe)]
    pub struct Tail<T: ?Sized> {
        #[scry(via(ArrayPolicy))]
        pub value: T,
    }
}

#[derive(Debug, PartialEq, FromNode, ToNode)]
struct Tree<T> {
    value: T,
    #[scry(default = Vec::new(), via(Delegate))]
    children: Vec<self::Tree<T>>,
}

#[derive(Debug, PartialEq, FromNode, ToNode)]
enum Branch<T> {
    Leaf(T),
    Children(#[scry(via(Delegate))] Vec<Self>),
}

#[derive(FromNode, ToNode, Describe)]
struct Hygiene<Policy, Result, Kind, Option> {
    #[scry(via(Policy))]
    value: std::sync::Mutex<Result>,
    #[scry(via(Delegate))]
    kind: Kind,
    #[scry(via(Delegate))]
    option: Option,
    #[scry(default = PhantomData, via(Marker))]
    policy: PhantomData<Policy>,
}

struct Locked;

impl<T: FromNode> FromNodeVia<std::sync::Mutex<T>> for Locked {
    fn from_node(node: &Node) -> Result<std::sync::Mutex<T>, NodeError> {
        Ok(std::sync::Mutex::new(node.as_type()?))
    }
}

impl<T: ToNode> ToNodeVia<std::sync::Mutex<T>> for Locked {
    fn to_node(value: &std::sync::Mutex<T>) -> Result<Node, NodeError> {
        value.lock().unwrap().to_node()
    }
}

impl<T: Describe> DescribeVia<std::sync::Mutex<T>> for Locked {
    fn describe() -> Desc {
        T::describe()
    }
}

struct Delegate;

impl<T: FromNode> FromNodeVia<T> for Delegate {
    fn from_node(node: &Node) -> Result<T, NodeError> {
        node.as_type()
    }
}

impl<T: ToNode + ?Sized> ToNodeVia<T> for Delegate {
    fn to_node(value: &T) -> Result<Node, NodeError> {
        value.to_node()
    }
}

impl<T: Describe + ?Sized> DescribeVia<T> for Delegate {
    fn describe() -> Desc {
        T::describe()
    }
}

struct Marker;

impl<T> FromNodeVia<PhantomData<T>> for Marker {
    fn from_node(node: &Node) -> Result<PhantomData<T>, NodeError> {
        match node.as_type::<Option<u16>>()? {
            None => Ok(PhantomData),
            Some(_) => Err(NodeError::invalid_value(&node.path, "marker must be null")),
        }
    }
}

impl<T> ToNodeVia<PhantomData<T>> for Marker {
    fn to_node(_value: &PhantomData<T>) -> Result<Node, NodeError> {
        ().to_node()
    }
}

impl<T> DescribeVia<PhantomData<T>> for Marker {
    fn describe() -> Desc {
        <()>::describe()
    }
}

struct Offset<const N: usize>;

impl<const N: usize> FromNodeVia<Wrapping<u16>> for Offset<N>
where
    [(); N]: OffsetAmount,
{
    fn from_node(node: &Node) -> Result<Wrapping<u16>, NodeError> {
        Ok(Wrapping(node.as_type::<u16>()? + <[(); N]>::AMOUNT))
    }
}

impl<const N: usize> ToNodeVia<Wrapping<u16>> for Offset<N>
where
    [(); N]: OffsetAmount,
{
    fn to_node(value: &Wrapping<u16>) -> Result<Node, NodeError> {
        (value.0 - <[(); N]>::AMOUNT).to_node()
    }
}

impl<const N: usize> DescribeVia<Wrapping<u16>> for Offset<N>
where
    [(); N]: OffsetAmount,
{
    fn describe() -> Desc {
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
    type Value;
}

struct Codec;

impl Encoding for Codec {
    type Value = std::sync::Mutex<u16>;
}

struct ArrayPolicy;

impl<const N: usize> FromNodeVia<[u16; N]> for ArrayPolicy {
    fn from_node(node: &Node) -> Result<[u16; N], NodeError> {
        node.as_type()
    }
}

impl<const N: usize> ToNodeVia<[u16; N]> for ArrayPolicy {
    fn to_node(value: &[u16; N]) -> Result<Node, NodeError> {
        value.to_node()
    }
}

impl ToNodeVia<[u16]> for ArrayPolicy {
    fn to_node(value: &[u16]) -> Result<Node, NodeError> {
        let views: Vec<_> = value.iter().collect();
        views.to_node()
    }
}

impl<const N: usize> DescribeVia<[u16; N]> for ArrayPolicy {
    fn describe() -> Desc {
        <[u16; N]>::describe()
    }
}

impl DescribeVia<[u16]> for ArrayPolicy {
    fn describe() -> Desc {
        <[u16]>::describe()
    }
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

mod selection {
    // ------------------------------------------------------------------------------------------ //

    pub type LockPolicy = super::Locked;
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
    let input: Mutex<InputOnly> = parse("#{ value: 17 }");
    assert_eq!(input.value.lock().unwrap().0, 17);

    let output = Mutex {
        value: std::sync::Mutex::new(OutputOnly(23)),
    }
    .to_node()
    .unwrap();
    assert_eq!(output.req::<u16>("value").unwrap(), 23);

    let DescKind::Struct { fields } = Mutex::<ShapeOnly>::describe().kind else {
        panic!("expected fields");
    };
    assert_eq!(fields[0].value.type_label(), "description only");
}

#[test]
fn policy_only_type_parameters_receive_their_operation_predicates() {
    let value: PolicyDependency<Locked> = parse("#{ value: 29 }");
    assert_eq!(*value.value.lock().unwrap(), 29);
    let output = value.to_node().unwrap();
    assert_eq!(output.req::<u16>("value").unwrap(), 29);
    assert_eq!(output.req::<Option<u16>>("marker").unwrap(), None);
    assert!(PolicyDependency::<Locked>::describe().validate_path("value").is_ok());
}

#[test]
fn policy_only_const_parameters_receive_conditional_operation_predicates() {
    let value: ConstPolicy<2> = parse("#{ value: 7 }");
    assert_eq!(value.value, Wrapping(9));
    assert_eq!(value.to_node().unwrap().req::<u16>("value").unwrap(), 7);
    let DescKind::Struct { fields } = ConstPolicy::<2>::describe().kind else {
        panic!("expected fields");
    };
    assert_eq!(fields[0].value.type_label(), "integer offset by 2");

    let value: ConstPolicy<3> = parse("#{ value: 7 }");
    assert_eq!(value.value, Wrapping(10));
    assert_eq!(value.to_node().unwrap().req::<u16>("value").unwrap(), 7);
}

#[test]
fn associated_and_qualified_same_name_targets_keep_selected_requirements() {
    let value: Value<Codec> = parse("#{ plain: 5, qualified: 8 }");
    assert_eq!(*value.plain.lock().unwrap(), 5);
    assert_eq!(*value.qualified.lock().unwrap(), 8);
    let output = value.to_node().unwrap();
    assert_eq!(output.req::<u16>("plain").unwrap(), 5);
    assert_eq!(output.req::<u16>("qualified").unwrap(), 8);
    let description = Value::<Codec>::describe();
    for name in ["plain", "qualified"] {
        assert!(description.validate_path(name).is_ok());
    }

    let value: Mutex<u16> = parse("#{ value: 11 }");
    assert_eq!(*value.value.lock().unwrap(), 11);
    assert_eq!(value.to_node().unwrap().req::<u16>("value").unwrap(), 11);
    assert!(Mutex::<u16>::describe().validate_path("value").is_ok());
}

#[test]
fn borrowed_lifetimes_and_unsized_output_and_descriptions_remain_supported() {
    let text = "borrowed text".to_owned();
    let value = Borrowed {
        value: text.as_str(),
    };
    assert_eq!(value.to_node().unwrap().req::<String>("value").unwrap(), text);
    assert!(Borrowed::<str>::describe().validate_path("value").is_ok());

    let sized: Tail<[u16; 2]> = parse("#{ value: [2, 7] }");
    let view: &Tail<[u16]> = &sized;
    assert_eq!(view.to_node().unwrap().req::<Vec<u16>>("value").unwrap(), [2, 7]);
    assert!(Tail::<[u16]>::describe().validate_path("value[0]").is_ok());
}

#[test]
fn recursive_adapted_targets_do_not_create_circular_impl_predicates() {
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
fn generic_policy_and_runtime_like_identifiers_preserve_the_callers_meaning() {
    let value: Hygiene<selection::LockPolicy, u16, u8, bool> =
        parse("#{ value: 13, kind: 4, option: true }");
    assert_eq!(*value.value.lock().unwrap(), 13);
    assert_eq!(value.kind, 4);
    assert!(value.option);
    let output = value.to_node().unwrap();
    assert_eq!(output.req::<u16>("value").unwrap(), 13);
    assert!(output.req::<bool>("option").unwrap());
    assert!(Hygiene::<selection::LockPolicy, u16, u8, bool>::describe()
        .validate_path("option")
        .is_ok());

    let input = Node::parse_str("#{ value: 13, kind: 4 }", Format::Rhai).unwrap();
    let error = match input.as_type::<Hygiene<Locked, u16, u8, bool>>() {
        Ok(_) => panic!("a type parameter named Option is still required"),
        Err(error) => error,
    };
    assert_eq!(error.path().unwrap().to_string(), "option");
}

// ---------------------------------------------------------------------------------------------- //

fn parse<T: FromNode>(source: &str) -> T {
    Node::parse_str(source, Format::Rhai).unwrap().as_type().unwrap()
}
