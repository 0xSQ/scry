use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod generate;
mod parse;

// ---------------------------------------------------------------------------------------------- //

/// Derives `FromNode` for parsing config from a Node tree.
///
/// Named structs and named enum payloads require maps. Present fields, including null, are
/// decoded as their complete Rust type. Only missing fields select `#[scry(default = EXPR)]`,
/// recursive `#[scry(from_defaults)]`, or implicit `None` for a written `Option<T>` type.
/// A type alias hiding `Option<T>` needs an explicit fallback to permit omission.
/// Each named object rejects unknown immediate keys after decoding its known fields. Child
/// decoders enforce their own shapes, and `allow_unknown_keys` applies only to its annotated struct.
///
/// `#[scry(from_node_with(parse))]` calls `parse(&Node)` for present fields and expects
/// `Result<FieldType, NodeError>`. For `Option<T>`, the hook returns `Option<T>` and receives
/// null too. Missing fields use their fallback without calling the hook. Leaf parsers should
/// use `Node::read_leaf` or typed decoding to consume input for explicit unread-input audits.
/// A strict map hook should validate its own keys with `Node::ensure_only_keys` or delegate to
/// a derived strict type.
/// Input errors use the Node's full logical path. Locationless hook errors gain that path without
/// changing their original cause. Errors that already carry a logical location are preserved.
/// Hooks also work on positional fields. Transparent newtypes decode their one complete field
/// directly, while tuples retain exact array arity. Positional renames and fallbacks are rejected.
/// Generic declarations and user constraints are preserved. Native generic field operations add
/// bounds on their complete types, while hooks replace the corresponding native requirements.
/// `#[scry(via(Policy))]` selects `Policy: FromNodeVia<FieldType>` for complete present values.
/// Via works in every field position and cannot be combined with operation hooks on that field.
#[proc_macro_derive(FromNode, attributes(scry))]
pub fn derive_from_node(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate::derive_from_node_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derives construction from Scry-declared defaults.
///
/// Named structs recursively apply their field policies. Enums require exactly one unit variant
/// marked with `#[scry(default)]`.
#[proc_macro_derive(FromDefaults, attributes(scry))]
pub fn derive_from_defaults(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate::derive_from_defaults_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derives `ToNode` for serializing config to a Node tree.
///
/// Every named field is serialized, including in enum payloads. `Option<T>::None` produces
/// null rather than omitting the key, preserving the value when a missing field has a default.
/// `#[scry(to_node_with(write))]` calls `write(&FieldType)` and expects `Result<Node, NodeError>`.
/// For `Option<T>`, the hook receives `&Option<T>` even when the value is `None`.
/// Error paths are relative to the value being serialized. Each field and enum payload prepends
/// its serialized key or index, including for hooks. Transparent newtypes preserve child paths.
/// Positional hooks receive references to the complete field type. Generic declarations and user
/// constraints are preserved, adding complete-field `ToNode` bounds only for native operations.
/// `#[scry(via(Policy))]` instead selects `Policy: ToNodeVia<FieldType>` and borrows that value.
#[proc_macro_derive(ToNode, attributes(scry))]
pub fn derive_to_node(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate::derive_to_node_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derives `FromStr` and `Display` for unit enums using Scry variant names.
#[proc_macro_derive(StringEnum, attributes(scry))]
pub fn derive_string_enum(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate::derive_string_enum_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derives `Describe` for generating configuration descriptions.
///
/// Delegates to each complete field type, preserving nested containers and nullable values.
/// Fields can replace that delegation with `#[scry(describe_with(function))]`. The function
/// returns a `Desc` for the complete value, including its nullability. Every other field type must
/// implement `Describe`. Missing implementations are compile-time errors.
/// Generic declarations and user constraints are preserved, adding complete-field `Describe`
/// bounds for native operations. Type and positional field prose override delegated prose only
/// when nonempty. Named field prose remains separate from its value's description.
/// `#[scry(via(Policy))]` selects `Policy: DescribeVia<FieldType>` without a native description
/// requirement. Adapted named fields retain omission metadata but suppress inferred target-domain
/// default displays and directly described enum default markers.
#[proc_macro_derive(Describe, attributes(scry))]
pub fn derive_describe(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate::derive_describe_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derives Scry's parsing and description traits for config types.
///
/// Named structs receive `FromNode`, `FromDefaults`, and `Describe`. Enums receive `FromNode` and
/// `Describe`, `FromDefaults` when one unit variant has `#[scry(default)]`, and string conversion
/// when requested with `#[scry(from_str)]`.
/// See [`FromNode`](macro@FromNode) for field fallback and parsing-hook rules.
#[proc_macro_derive(Config, attributes(scry))]
pub fn derive_config(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate::derive_config_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
