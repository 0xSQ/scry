use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod generate;
mod parse;

// ---------------------------------------------------------------------------------------------- //

/// Derives `FromNode` for parsing config from a Node tree.
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
#[proc_macro_derive(Config, attributes(scry))]
pub fn derive_config(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate::derive_config_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
