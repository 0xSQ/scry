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

/// Derives both `FromNode` and `Describe` for config types.
///
/// This is the recommended derive for config types that need both parsing
/// and description support for `--desc` CLI.
#[proc_macro_derive(Config, attributes(scry))]
pub fn derive_config(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate::derive_config_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
