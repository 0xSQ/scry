//! Checks preserved generics and operation-specific derive requirements.

use scry::desc::DescKind;
use scry::node::Format;
use scry::{
    Config, Desc, Describe, FromDefaults, FromNode, KeyPath, Node, NodeError, StringEnum, ToNode,
};

// ---------------------------------------------------------------------------------------------- //

/// A fixed collection of samples.
#[derive(Debug, PartialEq, Config, ToNode)]
struct GenericNamed<T = u16, const N: usize = 2>
where
    T: PartialEq,
{
    /// The authored samples.
    values: [T; N],
    #[scry(default = N)]
    limit: usize,
}

#[derive(Debug, PartialEq, FromNode, ToNode, Describe)]
struct GenericTuple<T, const N: usize>(T, [u16; N]);

#[derive(Debug, PartialEq, FromNode, ToNode, Describe)]
struct GenericNewtype<T>(T);

#[derive(Debug, PartialEq, Config, ToNode)]
enum GenericChoice<T> {
    #[scry(default)]
    Idle,
    One(T),
    Pair(T, Option<T>),
    Named {
        value: T,
    },
}

#[derive(FromDefaults)]
enum GenericDefault<T> {
    #[scry(default)]
    Idle,
    Value(T),
}

#[derive(Debug, PartialEq, FromNode, ToNode)]
struct Tree<T> {
    value: T,
    #[scry(default = Vec::new())]
    children: Vec<self::Tree<T>>,
}

#[derive(Debug, PartialEq, FromNode, ToNode)]
enum RecursivePayload<T> {
    Leaf(T),
    Branch(Vec<Self>),
}

#[derive(Config, ToNode)]
struct Associated<T: Encoding> {
    values: Vec<T::Value>,
    #[scry(default = 0)]
    retries: u8,
}

#[derive(Config, ToNode)]
struct Value<T: Encoding> {
    plain: T::Value,
    qualified: <T as Encoding>::Value,
}

#[derive(FromNode, ToNode, Describe)]
struct Envelope<T> {
    inner: external::Envelope<T>,
}

#[derive(ToNode, Describe)]
struct Borrowed<'a, T: ?Sized> {
    value: &'a T,
}

#[derive(FromNode)]
struct BorrowedInput<'a> {
    #[scry(from_node_with(read_borrowed))]
    value: &'a str,
}

#[derive(Debug, PartialEq, FromNode, FromDefaults)]
struct RecursiveDefault<T> {
    #[scry(from_defaults)]
    value: T,
}

#[derive(Debug, PartialEq, FromNode)]
struct ExplicitDefault<T: Wire> {
    #[scry(default = T::fallback(), from_node_with(read_wire::<T>))]
    value: T,
}

#[derive(FromNode)]
struct InputHook<T: Wire> {
    #[scry(from_node_with(read_wire::<T>))]
    value: T,
}

#[derive(ToNode)]
struct OutputHook<T: Wire> {
    #[scry(to_node_with(write_wire::<T>))]
    value: T,
}

#[derive(Describe)]
struct DescriptionHook<T: Wire> {
    #[scry(describe_with(describe_wire::<T>))]
    value: T,
}

#[derive(Config, ToNode)]
struct CombinedHooks<T: Wire> {
    #[scry(
        default = T::fallback(),
        from_node_with(read_wire::<T>),
        to_node_with(write_wire::<T>),
        describe_with(describe_wire::<T>)
    )]
    value: T,
}

#[derive(Debug, PartialEq, Config, ToNode)]
#[scry(from_str)]
enum ConstMode<const N: usize> {
    #[scry(default)]
    Idle,
    Active,
}

#[derive(Debug, PartialEq, StringEnum)]
enum StandaloneMode<const N: usize> {
    Idle,
    Active,
}

trait Encoding {
    type Value;
}

struct Codec;

impl Encoding for Codec {
    type Value = u16;
}

trait Wire: Sized {
    fn read(node: &Node) -> Result<Self, NodeError>;
    fn write(&self) -> Result<Node, NodeError>;
    fn description() -> Desc;
    fn fallback() -> Self;
}

#[derive(Debug, PartialEq)]
struct Opaque(u16);

impl Wire for Opaque {
    fn read(node: &Node) -> Result<Self, NodeError> {
        Ok(Self(node.as_type()?))
    }

    fn write(&self) -> Result<Node, NodeError> {
        self.0.to_node()
    }

    fn description() -> Desc {
        Desc::plain("encoded integer")
    }

    fn fallback() -> Self {
        Self(31)
    }
}

#[derive(Debug, PartialEq)]
struct ScryDefaultOnly(u16);

impl FromNode for ScryDefaultOnly {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        Ok(Self(node.as_type()?))
    }
}

impl FromDefaults for ScryDefaultOnly {
    fn from_defaults_at(_path: &KeyPath) -> Result<Self, NodeError> {
        Ok(Self(47))
    }
}

mod external {
    use scry::{Describe, FromNode, ToNode};

    // ------------------------------------------------------------------------------------------ //

    #[derive(FromNode, ToNode, Describe)]
    pub struct Envelope<T>(pub T);
}

// ---------------------------------------------------------------------------------------------- //

#[test]
fn named_config_preserves_parameter_defaults_const_generics_and_where_clauses() {
    let value: GenericNamed = parse("#{ values: [5, 8] }");
    assert_eq!(value.values, [5, 8]);
    assert_eq!(value.limit, 2);
    assert_eq!(value.to_node().unwrap().as_type::<GenericNamed>().unwrap(), value);

    let value: GenericNamed<u8, 3> = parse("#{ values: [1, 2, 3] }");
    assert_eq!(value.values, [1, 2, 3]);
    assert_eq!(value.limit, 3);
    let description = GenericNamed::<u8, 3>::describe();
    assert!(description.validate_path("values[2]").is_ok());
    assert_eq!(description.doc, "A fixed collection of samples.");
    let DescKind::Struct { fields } = description.kind else {
        panic!("expected fields");
    };
    assert_eq!(fields[0].doc, "The authored samples.");

    let error = scry::from_defaults::<GenericNamed>().unwrap_err();
    assert_eq!(error.path(), Some(&KeyPath::from_keys(["values"])));
}

#[test]
fn tuple_and_transparent_generic_structs_keep_their_wire_shapes() {
    let pair: GenericTuple<String, 2> = parse("[\"label\", [2, 4]]");
    assert_eq!(pair, GenericTuple("label".to_owned(), [2, 4]));
    assert_eq!(pair.to_node().unwrap().as_type::<GenericTuple<String, 2>>().unwrap(), pair);
    GenericTuple::<String, 2>::describe().validate_path("[1][0]").unwrap();

    let value: GenericNewtype<Vec<u16>> = parse("[2, 4]");
    assert_eq!(value.0, [2, 4]);
    assert_eq!(value.to_node().unwrap().as_type::<Vec<u16>>().unwrap(), [2, 4]);
    assert_eq!(GenericNewtype::<Vec<u16>>::describe().type_label(), "list[u16]");
}

#[test]
fn generic_enums_support_all_payload_shapes_and_independent_defaults() {
    let cases = [
        ("\"idle\"", GenericChoice::Idle),
        ("#{ one: 7 }", GenericChoice::One(7u16)),
        ("#{ pair: [7, ()] }", GenericChoice::Pair(7, None)),
        ("#{ named: #{ value: 7 } }", GenericChoice::Named { value: 7 }),
    ];
    for (input, expected) in cases {
        let value: GenericChoice<u16> = parse(input);
        assert_eq!(value, expected);
        assert_eq!(value.to_node().unwrap().as_type::<GenericChoice<u16>>().unwrap(), value);
    }
    let description = GenericChoice::<u16>::describe();
    for path in ["one", "pair[1]", "named.value"] {
        description.validate_path(path).unwrap();
    }
    assert_eq!(scry::from_defaults::<GenericChoice<u16>>().unwrap(), GenericChoice::Idle);

    assert!(matches!(
        scry::from_defaults::<GenericDefault<Opaque>>().unwrap(),
        GenericDefault::Idle
    ));
    let payload = GenericDefault::Value(Opaque(9));
    let GenericDefault::Value(value) = payload else {
        panic!("expected payload");
    };
    assert_eq!(value.0, 9);
}

#[test]
fn native_associated_fields_require_the_value_traits_without_constraining_the_owner() {
    let value: Associated<Codec> = parse("#{ values: [2, 6] }");
    assert_eq!(value.values, [2, 6]);
    assert_eq!(value.retries, 0);
    let output = value.to_node().unwrap();
    assert_eq!(output.req::<Vec<u16>>("values").unwrap(), [2, 6]);
    assert_eq!(
        Associated::<Codec>::describe().entry_at_path("values[0]").unwrap().display().trim(),
        "◆ 0: u16"
    );

    let value: Value<Codec> = parse("#{ plain: 3, qualified: 7 }");
    assert_eq!(value.plain, 3);
    assert_eq!(value.qualified, 7);
    let output = value.to_node().unwrap();
    assert_eq!(output.req::<u16>("plain").unwrap(), 3);
    assert_eq!(output.req::<u16>("qualified").unwrap(), 7);
    let description = Value::<Codec>::describe();
    for field in ["plain", "qualified"] {
        description.validate_path(field).unwrap();
    }

    let value: Envelope<u16> = parse("#{ inner: 11 }");
    assert_eq!(value.inner.0, 11);
    let output = value.to_node().unwrap();
    assert_eq!(output.req::<u16>("inner").unwrap(), 11);
    assert_eq!(output.as_type::<Envelope<u16>>().unwrap().inner.0, 11);
    let description = Envelope::<u16>::describe();
    let DescKind::Struct { fields } = description.kind else {
        panic!("expected fields");
    };
    assert_eq!(fields[0].value.type_label(), "u16");
}

#[test]
fn borrowed_lifetimes_and_unsized_parameters_are_preserved() {
    let text = "borrowed value".to_owned();
    let value = Borrowed {
        value: text.as_str(),
    };
    assert_eq!(value.to_node().unwrap().req::<String>("value").unwrap(), text);
    assert!(Borrowed::<str>::describe().validate_path("value").is_ok());

    let input: BorrowedInput<'_> = parse("#{ value: true }");
    assert_eq!(input.value, "enabled");
}

#[test]
fn recursive_generic_fields_do_not_create_circular_impl_predicates() {
    let value: Tree<u16> = parse("#{ value: 1, children: [#{ value: 2 }] }");
    assert_eq!(
        value.children,
        [Tree {
            value: 2,
            children: Vec::new()
        }]
    );
    assert_eq!(value.to_node().unwrap().as_type::<Tree<u16>>().unwrap(), value);

    let value: RecursivePayload<u16> =
        parse("#{ branch: [#{ leaf: 1 }, #{ branch: [#{ leaf: 2 }] }] }");
    assert_eq!(
        value,
        RecursivePayload::Branch(vec![
            RecursivePayload::Leaf(1),
            RecursivePayload::Branch(vec![RecursivePayload::Leaf(2)])
        ])
    );
    assert_eq!(value.to_node().unwrap().as_type::<RecursivePayload<u16>>().unwrap(), value);
}

#[test]
fn generic_scry_defaults_do_not_require_rust_default() {
    let value = scry::from_defaults::<RecursiveDefault<ScryDefaultOnly>>().unwrap();
    assert_eq!(value.value, ScryDefaultOnly(47));
    let value: RecursiveDefault<ScryDefaultOnly> = parse("#{ value: 8 }");
    assert_eq!(value.value, ScryDefaultOnly(8));

    let explicit: ExplicitDefault<Opaque> = parse("#{}");
    assert_eq!(explicit.value, Opaque(31));
    let explicit: ExplicitDefault<Opaque> = parse("#{ value: 8 }");
    assert_eq!(explicit.value, Opaque(8));
}

#[test]
fn generic_hooks_replace_only_their_operation_requirements() {
    let input: InputHook<Opaque> = parse("#{ value: 13 }");
    assert_eq!(input.value, Opaque(13));
    let output = OutputHook { value: Opaque(19) }.to_node().unwrap();
    assert_eq!(output.req::<u16>("value").unwrap(), 19);
    let description = DescriptionHook::<Opaque>::describe();
    let DescKind::Struct { fields } = description.kind else {
        panic!("expected fields");
    };
    assert_eq!(fields[0].value.type_label(), "encoded integer");
    let value = DescriptionHook { value: Opaque(23) };
    assert_eq!(value.value, Opaque(23));

    let combined: CombinedHooks<Opaque> = parse("#{ value: 29 }");
    assert_eq!(combined.value, Opaque(29));
    assert_eq!(combined.to_node().unwrap().req::<u16>("value").unwrap(), 29);
    assert!(CombinedHooks::<Opaque>::describe().validate_path("value").is_ok());
    assert_eq!(scry::from_defaults::<CombinedHooks<Opaque>>().unwrap().value, Opaque(31));
}

#[test]
fn generated_string_traits_preserve_const_parameters() {
    assert_eq!("active".parse::<ConstMode<3>>().unwrap(), ConstMode::Active);
    assert_eq!(ConstMode::<3>::Active.to_string(), "active");
    assert_eq!(parse::<ConstMode<3>>("\"active\""), ConstMode::Active);
    assert_eq!(scry::from_defaults::<ConstMode<3>>().unwrap(), ConstMode::Idle);
    assert_eq!(ConstMode::<3>::Idle.to_node().unwrap().as_type::<String>().unwrap(), "idle");
    assert_eq!("active".parse::<StandaloneMode<7>>().unwrap(), StandaloneMode::Active);
    assert_eq!(StandaloneMode::<7>::Idle.to_string(), "idle");
}

// ---------------------------------------------------------------------------------------------- //

fn parse<T: FromNode>(source: &str) -> T {
    Node::parse_str(source, Format::Rhai).unwrap().as_type().unwrap()
}

fn read_borrowed(node: &Node) -> Result<&'static str, NodeError> {
    Ok(if node.as_type::<bool>()? {
        "enabled"
    } else {
        "disabled"
    })
}

fn read_wire<T: Wire>(node: &Node) -> Result<T, NodeError> {
    T::read(node)
}

fn write_wire<T: Wire>(value: &T) -> Result<Node, NodeError> {
    value.write()
}

fn describe_wire<T: Wire>() -> Desc {
    T::description()
}
