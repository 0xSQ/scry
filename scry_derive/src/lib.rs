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

/// Derives `DefaultNode` for generating default baselines.
#[proc_macro_derive(DefaultNode, attributes(scry, default))]
pub fn derive_default_node(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate::derive_default_node_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derives `FromNode`, `Describe`, `ToNode`, and `DefaultNode` for config types.
///
/// This is the recommended derive for config types. It produces everything the
/// `Setup` CLI builder needs: parsing, description support for `--desc`, node
/// serialization, and the default baselines that `--get` uses to annotate overrides.
#[proc_macro_derive(Config, attributes(scry, default))]
pub fn derive_config(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate::derive_config_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
