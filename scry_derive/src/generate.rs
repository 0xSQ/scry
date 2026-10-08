//! Code generation for derive macros: FromNode, ToNode, Describe, Config.

use std::collections::HashSet;

use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{DeriveInput, Generics, Member};

use crate::parse::{
    self, is_option_type, rename_all_variant, DeriveTarget, EnumInfo, FieldFallback, FieldInfo,
    StructFields, StructInfo, VariantData,
};

// ---------------------------------------------------------------------------------------------- //
// Public Entry Points

/// Generates `FromNode` implementation for parsing.
pub fn derive_from_node_impl(input: &DeriveInput) -> syn::Result<TokenStream> {
    match parse::parse_input(input)? {
        DeriveTarget::Struct(info) => generate_struct_from_node(&info),
        DeriveTarget::Enum(info) => generate_enum_from_node(&info),
    }
}

/// Generates `FromDefaults` for recursive construction from Scry field defaults.
pub fn derive_from_defaults_impl(input: &DeriveInput) -> syn::Result<TokenStream> {
    match parse::parse_input(input)? {
        DeriveTarget::Struct(info) => generate_struct_from_defaults(&info),
        DeriveTarget::Enum(info) => generate_enum_from_defaults(&info, true),
    }
}

/// Generates `ToNode` implementation for serialization.
pub fn derive_to_node_impl(input: &DeriveInput) -> syn::Result<TokenStream> {
    match parse::parse_input(input)? {
        DeriveTarget::Struct(info) => generate_struct_to_node(&info),
        DeriveTarget::Enum(info) => generate_enum_to_node(&info),
    }
}

/// Generates `FromStr` and `Display` implementations for unit enums.
pub fn derive_string_enum_impl(input: &DeriveInput) -> syn::Result<TokenStream> {
    match parse::parse_input(input)? {
        DeriveTarget::Struct(_) => {
            Err(syn::Error::new_spanned(input, "StringEnum can only be derived for enums"))
        }
        DeriveTarget::Enum(info) => generate_enum_string_enum(&info),
    }
}

/// Generates `Describe` implementation for documentation.
pub fn derive_describe_impl(input: &DeriveInput) -> syn::Result<TokenStream> {
    match parse::parse_input(input)? {
        DeriveTarget::Struct(info) => generate_struct_describe(&info),
        DeriveTarget::Enum(info) => generate_enum_describe(&info),
    }
}

/// Generates both `FromNode` and `Describe` implementations.
///
/// Produces `FromNode` for parsing and `Describe` for documentation.
/// This is the recommended derive for config types.
pub fn derive_config_impl(input: &DeriveInput) -> syn::Result<TokenStream> {
    let (from_node, desc, from_defaults, string_enum) = match parse::parse_input(input)? {
        DeriveTarget::Struct(info) => {
            let defaults = if matches!(&info.fields, StructFields::Named(_)) {
                generate_struct_from_defaults(&info)?
            } else {
                quote! {}
            };
            (
                generate_struct_from_node(&info)?,
                generate_struct_describe(&info)?,
                defaults,
                quote! {},
            )
        }
        DeriveTarget::Enum(info) => {
            let from_defaults = generate_enum_from_defaults(&info, false)?;
            let string_enum = if info.attrs.from_str {
                generate_enum_string_enum(&info)?
            } else {
                quote! {}
            };
            (
                generate_enum_from_node(&info)?,
                generate_enum_describe(&info)?,
                from_defaults,
                string_enum,
            )
        }
    };
    Ok(quote! {
        #from_node
        #desc
        #from_defaults
        #string_enum
    })
}

// ---------------------------------------------------------------------------------------------- //
// FromNode Generation

fn generate_struct_from_defaults(info: &StructInfo) -> syn::Result<TokenStream> {
    let struct_name = &info.ident;

    if !matches!(&info.fields, StructFields::Named(_)) {
        return Err(syn::Error::new_spanned(
            struct_name,
            "FromDefaults can only be derived for named structs",
        ));
    }

    let scry = scry_crate_path();
    let mut generics = info.generics.clone();
    generics.make_where_clause().predicates.push(syn::parse_quote!(Self: #scry::FromNode));
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics #scry::FromDefaults for #struct_name #type_generics #where_clause {
            fn from_defaults_at(
                path: &#scry::KeyPath,
            ) -> ::core::result::Result<Self, #scry::NodeError> {
                <Self as #scry::FromNode>::from_node(
                    &#scry::Node::empty_map_at(path.clone()),
                )
            }
        }
    })
}

fn generate_enum_from_defaults(info: &EnumInfo, require_default: bool) -> syn::Result<TokenStream> {
    let enum_name = &info.ident;
    let default_variant = info.variants.iter().find(|variant| variant.attrs.is_default);

    let Some(default_variant) = default_variant else {
        if require_default {
            return Err(syn::Error::new_spanned(
                enum_name,
                "deriving `FromDefaults` for an enum requires exactly one unit variant marked \
                 `#[scry(default)]`",
            ));
        }
        return Ok(quote! {});
    };

    let scry = scry_crate_path();
    let variant_name = &default_variant.ident;
    let (impl_generics, type_generics, where_clause) = info.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics #scry::FromDefaults for #enum_name #type_generics #where_clause {
            fn from_defaults_at(
                _path: &#scry::KeyPath,
            ) -> ::core::result::Result<Self, #scry::NodeError> {
                Ok(Self::#variant_name)
            }
        }
    })
}

fn generate_struct_from_node(info: &StructInfo) -> syn::Result<TokenStream> {
    let scry = scry_crate_path();
    let struct_name = &info.ident;
    let node = private_ident("node");
    let arr = private_ident("array");
    let generics = struct_generics(info, Operation::FromNode, &scry);
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    match &info.fields {
        StructFields::Named(fields) => {
            let field_parsers: Vec<TokenStream> = fields
                .iter()
                .map(|field| generate_field_parser(field, &quote! { #node }))
                .collect();

            let validate_keys = if info.allow_unknown_keys {
                quote! {}
            } else {
                let keys: Vec<String> = fields.iter().map(FieldInfo::config_key).collect();
                quote! { #node.ensure_only_keys(&[#(#keys),*])?; }
            };

            Ok(quote! {
                impl #impl_generics #scry::FromNode for #struct_name #type_generics #where_clause {
                    fn from_node(#node: &#scry::Node) -> ::core::result::Result<Self, #scry::NodeError> {
                        #node.as_map()?;
                        let result = Self {
                            #(#field_parsers),*
                        };
                        #validate_keys
                        Ok(result)
                    }
                }
            })
        }
        StructFields::Tuple(fields) => {
            if fields.len() == 1 {
                // Newtype: parse as inner type directly
                let value = generate_present_field_parser(&fields[0], &quote! { #node }, &scry);
                Ok(quote! {
                    impl #impl_generics #scry::FromNode for #struct_name #type_generics #where_clause {
                        fn from_node(#node: &#scry::Node) -> ::core::result::Result<Self, #scry::NodeError> {
                            Ok(Self(#value?))
                        }
                    }
                })
            } else {
                // Tuple: parse from array
                let field_count = fields.len();
                let field_parsers: Vec<TokenStream> = fields
                    .iter()
                    .enumerate()
                    .map(|(index, field)| {
                        let input = quote! { &#arr[#index] };
                        let value = generate_present_field_parser(field, &input, &scry);
                        quote! { #value? }
                    })
                    .collect();

                Ok(quote! {
                    impl #impl_generics #scry::FromNode for #struct_name #type_generics #where_clause {
                        fn from_node(#node: &#scry::Node) -> ::core::result::Result<Self, #scry::NodeError> {
                            let #arr = #node.as_vec()?;
                            if #arr.len() != #field_count {
                                return Err(#scry::NodeError::array_length(&#node.path, #field_count, #arr.len()));
                            }
                            Ok(Self(
                                #(#field_parsers),*
                            ))
                        }
                    }
                })
            }
        }
    }
}

fn generate_field_parser(field: &FieldInfo, parent: &TokenStream) -> TokenStream {
    let field_name = &field.member;
    let value = generate_field_value_parser(field, parent);

    quote! {
        #field_name: #value
    }
}

/// Generates complete-field decoding with a separate missing-field fallback.
fn generate_field_value_parser(field: &FieldInfo, parent: &TokenStream) -> TokenStream {
    let scry = scry_crate_path();
    let ty = &field.ty;
    let key = field.config_key();
    let input = Ident::new("__scry_field_node", Span::mixed_site());
    let present = generate_present_field_parser(field, &quote! { #input }, &scry);
    let missing = match &field.attrs.fallback {
        FieldFallback::Expression(expr) => quote! { #expr },
        FieldFallback::FromDefaults => quote! {
            <#ty as #scry::FromDefaults>::from_defaults_at(&#parent.path.push_key(#key))?
        },
        FieldFallback::Unspecified if is_option_type(ty) => quote! { None },
        FieldFallback::Unspecified => quote! {
            return Err(#scry::NodeError::missing_required(&#parent.path.push_key(#key)))
        },
    };

    quote! {
        match #parent.opt_node(#scry::KeyPath::from_keys([#key]))? {
            Some(#input) => #present?,
            None => #missing,
        }
    }
}

fn generate_enum_from_node(info: &EnumInfo) -> syn::Result<TokenStream> {
    let scry = scry_crate_path();
    let enum_name = &info.ident;
    let node = private_ident("node");
    let payload = private_ident("payload");
    let arr = private_ident("array");
    let map = private_ident("input_map");
    let variant_key = private_ident("variant_key");
    let generics = enum_generics(info, Operation::FromNode, &scry);
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    // Collect variant info
    let mut unit_variants: Vec<(String, &syn::Ident)> = Vec::new();
    let mut payload_variants: Vec<(String, &syn::Ident, &VariantData)> = Vec::new();
    let mut all_variant_keys: Vec<String> = Vec::new();

    for variant in &info.variants {
        let v_ident = &variant.ident;
        let key = variant
            .attrs
            .rename
            .clone()
            .unwrap_or_else(|| rename_all_variant(&v_ident.to_string(), info.attrs.rename_all));
        all_variant_keys.push(key.clone());

        match &variant.data {
            VariantData::Unit => {
                unit_variants.push((key, v_ident));
            }
            data => {
                payload_variants.push((key, v_ident, data));
            }
        }
    }

    let expected_str = all_variant_keys.join(", ");

    // Generate string match arms for unit variants
    // Use lowercased keys since input is lowercased before matching
    let unit_string_arms: Vec<TokenStream> = unit_variants
        .iter()
        .map(|(key, v_ident)| {
            let spellings: Vec<String> = parse::variant_spellings(key)
                .into_iter()
                .map(|name| name.to_ascii_lowercase())
                .collect();
            quote! { #(#spellings)|* => Ok(#enum_name::#v_ident) }
        })
        .collect();

    // Generate error arms for payload variants used as strings
    // Use lowercased keys for matching, original keys for error messages
    let mut string_spellings: HashSet<String> = unit_variants
        .iter()
        .flat_map(|(key, _)| parse::variant_spellings(key))
        .map(|name| name.to_ascii_lowercase())
        .collect();
    let payload_string_error_arms: Vec<TokenStream> = payload_variants
        .iter()
        .filter_map(|(key, _, _)| {
            let spellings: Vec<String> = parse::variant_spellings(key)
                .into_iter()
                .map(|name| name.to_ascii_lowercase())
                .filter(|name| string_spellings.insert(name.clone()))
                .collect();
            if spellings.is_empty() {
                return None;
            }
            let msg = format!(
                "variant '{}' requires a payload - use {{\"{}\": <value>}} syntax",
                key, key
            );
            Some(quote! {
                #(#spellings)|* => return Err(#scry::NodeError::invalid_value(&#node.path, #msg))
            })
        })
        .collect();

    // Generate map match arms for payload variants
    let payload_map_arms: Vec<TokenStream> = payload_variants
        .iter()
        .map(|(key, v_ident, data)| {
            let parse_payload = match data {
                VariantData::Unit => unreachable!(),
                VariantData::Tuple(fields) if fields.len() == 1 => {
                    // Single-field tuple: payload is the value directly
                    // Parse first, then add helpful hint if it fails on a 1-element array
                    let hint_msg = format!(
                        "hint: payload is a 1-element array - if you meant a single value, \
                        use {{\"{}\": <value>}} instead of {{\"{}\": [<value>]}}",
                        key, key
                    );
                    let value = generate_present_field_parser(&fields[0], &quote! { #payload }, &scry);
                    quote! {
                        match #value {
                            Ok(v) => Ok(#enum_name::#v_ident(v)),
                            Err(e) => {
                                // Add hint for common "accidental brackets" mistake
                                if let Some(#arr) = #payload.as_opt_vec() {
                                    if #arr.len() == 1 {
                                        return Err(#scry::NodeError::invalid_value_with_source(
                                            &#payload.path,
                                            format!("failed to parse variant '{}': {} ({})", #key, e, #hint_msg),
                                            e,
                                        ));
                                    }
                                }
                                Err(e)
                            }
                        }
                    }
                }
                VariantData::Tuple(fields) => {
                    // Multi-field tuple: payload is an array
                    let field_count = fields.len();
                    let field_parsers: Vec<TokenStream> = fields.iter().enumerate().map(|(index, field)| {
                        let value = generate_present_field_parser(field, &quote! { &#arr[#index] }, &scry);
                        quote! { #value? }
                    }).collect();

                    quote! {
                        let #arr = #payload.as_vec()?;
                        if #arr.len() != #field_count {
                            return Err(#scry::NodeError::array_length(&#payload.path, #field_count, #arr.len()));
                        }
                        Ok(#enum_name::#v_ident(#(#field_parsers),*))
                    }
                }
                VariantData::Struct(fields) => {
                    // Struct variant: payload is a map with field semantics
                    let field_parsers: Vec<TokenStream> =
                        fields.iter().map(|field| generate_field_value_parser(field, &quote! { #payload })).collect();
                    let field_names: Vec<&Member> = fields.iter().map(|f| &f.member).collect();
                    let field_keys: Vec<String> = fields
                        .iter()
                        .map(FieldInfo::config_key)
                        .collect();

                    quote! {
                        #payload.as_map()?;
                        let result = #enum_name::#v_ident {
                            #(#field_names: #field_parsers),*
                        };
                        #payload.ensure_only_keys(&[#(#field_keys),*])?;
                        Ok(result)
                    }
                }
            };

            let spellings = parse::variant_spellings(key);
            quote! { #(#spellings)|* => { #parse_payload } }
        })
        .collect();

    // Generate error arms for unit variants used as map keys
    let mut map_spellings: HashSet<String> =
        payload_variants.iter().flat_map(|(key, _, _)| parse::variant_spellings(key)).collect();
    let unit_map_error_arms: Vec<TokenStream> = unit_variants
        .iter()
        .filter_map(|(key, _)| {
            let spellings: Vec<String> = parse::variant_spellings(key)
                .into_iter()
                .filter(|name| map_spellings.insert(name.clone()))
                .collect();
            if spellings.is_empty() {
                return None;
            }
            let msg = format!(
                "unit variant '{}' must be written as a string \"{}\", not as {{\"{}\":...}}",
                key, key, key
            );
            Some(quote! {
                #(#spellings)|* => return Err(#scry::NodeError::invalid_value(&#node.path, #msg))
            })
        })
        .collect();

    Ok(quote! {
        impl #impl_generics #scry::FromNode for #enum_name #type_generics #where_clause {
            fn from_node(#node: &#scry::Node) -> ::core::result::Result<Self, #scry::NodeError> {


                match &#node.kind {
                    // String form: only unit variants allowed
                    #scry::node::Kind::Leaf(_) => {
                        let value = #node.read_leaf("string or map")?;
                        if let #scry::node::Value::String(s) = value {
                            match s.to_ascii_lowercase().as_str() {
                                #(#unit_string_arms,)*
                                #(#payload_string_error_arms,)*
                                other => return Err(#scry::NodeError::invalid_value(
                                    &#node.path,
                                    format!(
                                        "unknown variant '{}' - expected one of: {}",
                                        other,
                                        #expected_str,
                                    ),
                                )),
                            }
                        } else {
                            return Err(#scry::NodeError::type_mismatch(
                                &#node.path,
                                "string or map",
                                value.type_name(),
                            ));
                        }
                    }

                    // Map form: exactly one key required
                    #scry::node::Kind::Map(#map) => {
                        if #map.is_empty() {
                            return Err(#scry::NodeError::invalid_value(
                                &#node.path,
                                format!("expected exactly one variant key, found empty map - expected one of: {}", #expected_str),
                            ));
                        }
                        if #map.len() > 1 {
                            let keys: ::std::vec::Vec<&str> = #map.keys().map(|s| s.as_str()).collect();
                            return Err(#scry::NodeError::invalid_value(
                                &#node.path,
                                format!("expected exactly one variant key, found {}: {}", #map.len(), keys.join(", ")),
                            ));
                        }

                        let (#variant_key, #payload) = #map.iter().next().unwrap();
                        match #variant_key.as_str() {
                            #(#payload_map_arms,)*
                            #(#unit_map_error_arms,)*
                            other => return Err(#scry::NodeError::invalid_value(
                                &#node.path,
                                format!(
                                    "unknown variant '{}' - expected one of: {}",
                                    other,
                                    #expected_str,
                                ),
                            )),
                        }
                    }

                    // Array form: not valid for enums
                    #scry::node::Kind::Vec(_) => {
                        return Err(#scry::NodeError::type_mismatch(
                            &#node.path,
                            "string or map",
                            "array",
                        ));
                    }
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------------------------- //
// ToNode Generation

fn generate_struct_to_node(info: &StructInfo) -> syn::Result<TokenStream> {
    let scry = scry_crate_path();
    let struct_name = &info.ident;
    let map = private_ident("output_map");
    let generics = struct_generics(info, Operation::ToNode, &scry);
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    match &info.fields {
        StructFields::Named(fields) => {
            let field_serializers: Vec<TokenStream> =
                fields.iter().map(|f| generate_field_serializer(f, &scry, &map)).collect();

            Ok(quote! {
                impl #impl_generics #scry::ToNode for #struct_name #type_generics #where_clause {
                    fn to_node(&self) -> ::core::result::Result<#scry::Node, #scry::NodeError> {
                        let mut #map = #scry::_private::IndexMap::new();
                        #(#field_serializers)*
                        Ok(#scry::Node {
                            path: #scry::KeyPath::new(),
                            kind: #scry::node::Kind::Map(#map),
                        })
                    }
                }
            })
        }
        StructFields::Tuple(fields) => {
            if fields.len() == 1 {
                // Newtype: delegate to inner
                let value = generate_field_output(&fields[0], &quote! { &self.0 }, &scry);
                Ok(quote! {
                    impl #impl_generics #scry::ToNode for #struct_name #type_generics #where_clause {
                        fn to_node(&self) -> ::core::result::Result<#scry::Node, #scry::NodeError> {
                            #value
                        }
                    }
                })
            } else {
                // Tuple: serialize as array
                let field_serializers: Vec<TokenStream> = fields
                    .iter()
                    .enumerate()
                    .map(|(index, field)| {
                        let member = &field.member;
                        let value = generate_field_output(field, &quote! { &self.#member }, &scry);
                        quote! {
                            #value.map_err(|error| error.prepend_path(&#scry::KeyPath::from_index(#index)))?
                        }
                    })
                    .collect();

                Ok(quote! {
                    impl #impl_generics #scry::ToNode for #struct_name #type_generics #where_clause {
                        fn to_node(&self) -> ::core::result::Result<#scry::Node, #scry::NodeError> {
                            let children = vec![
                                #(#field_serializers),*
                            ];
                            Ok(#scry::Node {
                                path: #scry::KeyPath::new(),
                                kind: #scry::node::Kind::Vec(children),
                            })
                        }
                    }
                })
            }
        }
    }
}

fn generate_field_serializer(field: &FieldInfo, scry: &TokenStream, map: &Ident) -> TokenStream {
    let field_name = &field.member;
    let key = field.config_key();
    let value = generate_field_output(field, &quote! { &self.#field_name }, scry);

    quote! {
        #map.insert(
            #key.to_string(),
            #value.map_err(|error| error.prepend_path(&#scry::KeyPath::from_keys([#key])))?,
        );
    }
}

/// Generates field serialization code for struct variant fields.
///
/// Similar to `generate_field_serializer` but uses bound variable names
/// and inserts into `inner_map` instead of `map`.
fn generate_struct_variant_field_serializer(
    field: &FieldInfo,
    scry: &TokenStream,
    variant_key: &str,
    binding: &Ident,
    map: &Ident,
) -> TokenStream {
    let key = field.config_key();
    let value = generate_field_output(field, &quote! { #binding }, scry);

    quote! {
        #map.insert(
            #key.to_string(),
            #value.map_err(|error| {
                error.prepend_path(&#scry::KeyPath::from_keys([#variant_key, #key]))
            })?,
        );
    }
}

fn generate_enum_to_node(info: &EnumInfo) -> syn::Result<TokenStream> {
    let scry = scry_crate_path();
    let enum_name = &info.ident;
    let map = private_ident("output_map");
    let inner_map = private_ident("payload_map");
    let inner = private_ident("inner");
    let generics = enum_generics(info, Operation::ToNode, &scry);
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let mut match_arms = Vec::new();

    for variant in &info.variants {
        let v_ident = &variant.ident;
        let key = variant
            .attrs
            .rename
            .clone()
            .unwrap_or_else(|| rename_all_variant(&v_ident.to_string(), info.attrs.rename_all));

        match &variant.data {
            VariantData::Unit => {
                // Unit variant: serialize as string
                match_arms.push(quote! {
                    #enum_name::#v_ident => {
                        Ok(#scry::Node {
                            path: #scry::KeyPath::new(),
                            kind: #scry::node::Kind::Leaf(#scry::node::Leaf::new(#scry::node::Value::String(#key.to_string()))),
                        })
                    }
                });
            }
            VariantData::Tuple(fields) if fields.len() == 1 => {
                // Single-field tuple: serialize as {"name": value}
                let value = generate_field_output(&fields[0], &quote! { #inner }, &scry);
                match_arms.push(quote! {
                    #enum_name::#v_ident(#inner) => {
                        let mut #map = #scry::_private::IndexMap::new();
                        #map.insert(
                            #key.to_string(),
                            #value.map_err(|error| {
                                error.prepend_path(&#scry::KeyPath::from_keys([#key]))
                            })?,
                        );
                        Ok(#scry::Node {
                            path: #scry::KeyPath::new(),
                            kind: #scry::node::Kind::Map(#map),
                        })
                    }
                });
            }
            VariantData::Tuple(fields) => {
                // Multi-field tuple: serialize as {"name": [v1, v2, ...]}
                let field_count = fields.len();
                let field_bindings: Vec<syn::Ident> =
                    (0..field_count).map(|i| private_ident(&format!("field_{i}"))).collect();
                let field_serializers: Vec<TokenStream> = fields
                    .iter()
                    .zip(&field_bindings)
                    .enumerate()
                    .map(|(index, (field, binding))| {
                        let value = generate_field_output(field, &quote! { #binding }, &scry);
                        quote! {
                            #value.map_err(|error| {
                                error.prepend_path(&#scry::KeyPath::from_keys([#key]).push_index(#index))
                            })?
                        }
                    })
                    .collect();

                match_arms.push(quote! {
                    #enum_name::#v_ident(#(#field_bindings),*) => {
                        let mut #map = #scry::_private::IndexMap::new();
                        let inner_vec = vec![#(#field_serializers),*];
                        #map.insert(#key.to_string(), #scry::Node {
                            path: #scry::KeyPath::new(),
                            kind: #scry::node::Kind::Vec(inner_vec),
                        });
                        Ok(#scry::Node {
                            path: #scry::KeyPath::new(),
                            kind: #scry::node::Kind::Map(#map),
                        })
                    }
                });
            }
            VariantData::Struct(fields) => {
                // Struct variant: serialize as {"name": {"f1": v1, "f2": v2}}
                // Serializes every complete field value and respects field renaming.
                let field_names: Vec<&Member> = fields.iter().map(|f| &f.member).collect();
                let bindings: Vec<Ident> =
                    (0..fields.len()).map(|i| private_ident(&format!("field_{i}"))).collect();
                let field_serializers: Vec<TokenStream> = fields
                    .iter()
                    .zip(&bindings)
                    .map(|(field, binding)| {
                        generate_struct_variant_field_serializer(
                            field, &scry, &key, binding, &inner_map,
                        )
                    })
                    .collect();

                match_arms.push(quote! {
                    #enum_name::#v_ident { #(#field_names: #bindings),* } => {
                        let mut #inner_map = #scry::_private::IndexMap::new();
                        #(#field_serializers)*
                        let mut #map = #scry::_private::IndexMap::new();
                        #map.insert(#key.to_string(), #scry::Node {
                            path: #scry::KeyPath::new(),
                            kind: #scry::node::Kind::Map(#inner_map),
                        });
                        Ok(#scry::Node {
                            path: #scry::KeyPath::new(),
                            kind: #scry::node::Kind::Map(#map),
                        })
                    }
                });
            }
        }
    }

    Ok(quote! {
        impl #impl_generics #scry::ToNode for #enum_name #type_generics #where_clause {
            fn to_node(&self) -> ::core::result::Result<#scry::Node, #scry::NodeError> {
                match self {
                    #(#match_arms)*
                }
            }
        }
    })
}

fn generate_enum_string_enum(info: &EnumInfo) -> syn::Result<TokenStream> {
    let scry = scry_crate_path();
    let enum_name = &info.ident;
    let (impl_generics, type_generics, where_clause) = info.generics.split_for_impl();
    let mut display_arms = Vec::new();
    let mut parse_arms = Vec::new();
    let mut expected = Vec::new();

    for variant in &info.variants {
        let v_ident = &variant.ident;
        let key = variant
            .attrs
            .rename
            .clone()
            .unwrap_or_else(|| rename_all_variant(&v_ident.to_string(), info.attrs.rename_all));

        if !matches!(variant.data, VariantData::Unit) {
            return Err(syn::Error::new_spanned(
                v_ident,
                "StringEnum only supports unit enum variants",
            ));
        }

        expected.push(key.clone());
        let spellings: Vec<String> = parse::variant_spellings(&key)
            .into_iter()
            .map(|name| name.to_ascii_lowercase())
            .collect();

        display_arms.push(quote! {
            #enum_name::#v_ident => f.write_str(#key)
        });
        parse_arms.push(quote! {
            #(#spellings)|* => Ok(#enum_name::#v_ident)
        });
    }

    let expected_str = expected.join(", ");

    Ok(quote! {
        impl #impl_generics std::str::FromStr for #enum_name #type_generics #where_clause {
            type Err = #scry::StringEnumError;

            fn from_str(s: &str) -> ::core::result::Result<Self, Self::Err> {
                match s.to_ascii_lowercase().as_str() {
                    #(#parse_arms,)*
                    _ => Err(#scry::StringEnumError::new(stringify!(#enum_name), s, #expected_str)),
                }
            }
        }

        impl #impl_generics std::fmt::Display for #enum_name #type_generics #where_clause {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    #(#display_arms,)*
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------------------------- //
// Describe Generation (generates Desc)

fn generate_struct_describe(info: &StructInfo) -> syn::Result<TokenStream> {
    let scry = scry_crate_path();
    let struct_name = &info.ident;
    let generics = struct_generics(info, Operation::Describe, &scry);
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let doc = doc_override(&info.doc);

    match &info.fields {
        StructFields::Named(fields) => {
            let field_descs: Vec<TokenStream> =
                fields.iter().map(|f| generate_field_desc(f, &scry)).collect();

            Ok(quote! {
                impl #impl_generics #scry::Describe for #struct_name #type_generics #where_clause {
                    fn describe() -> #scry::Desc {
                        #scry::Desc::structure(vec![
                            #(#field_descs),*
                        ]) #doc
                    }
                }
            })
        }
        StructFields::Tuple(fields) => {
            if fields.len() == 1 {
                // Newtype: desc of the inner type
                let value = generate_positional_desc(&fields[0], &scry);
                Ok(quote! {
                    impl #impl_generics #scry::Describe for #struct_name #type_generics #where_clause {
                        fn describe() -> #scry::Desc {
                            (#value) #doc
                        }
                    }
                })
            } else {
                // Tuple: desc of each element
                let elem_descs: Vec<TokenStream> =
                    fields.iter().map(|field| generate_positional_desc(field, &scry)).collect();
                Ok(quote! {
                    impl #impl_generics #scry::Describe for #struct_name #type_generics #where_clause {
                        fn describe() -> #scry::Desc {
                            #scry::Desc::tuple(vec![#(#elem_descs),*]) #doc
                        }
                    }
                })
            }
        }
    }
}

fn generate_field_desc(field: &FieldInfo, scry: &TokenStream) -> TokenStream {
    let field_name = field.config_key();
    let doc = &field.doc;
    let ty = &field.ty;

    // Determine optionality:
    // - Option<T> is optional (implicit None)
    // - An explicit expression or recursive defaults make it optional
    let is_optional =
        is_option_type(ty) || !matches!(field.attrs.fallback, FieldFallback::Unspecified);

    // Only simple literals are honest config-oriented display values. Arbitrary Rust syntax is
    // construction machinery rather than user-facing configuration documentation.
    let default_display = match &field.attrs.fallback {
        _ if field.attrs.with.is_some() => quote! {},
        FieldFallback::Expression(expr) => literal_default_display(expr)
            .map(|display| quote! { .with_default(#display) })
            .unwrap_or_default(),
        FieldFallback::Unspecified | FieldFallback::FromDefaults => quote! {},
    };

    // Mark as optional if needed
    let optional_expr = if is_optional && default_display.is_empty() {
        quote! { .optional() }
    } else {
        quote! {}
    };

    // Custom desc function overrides normal type-based desc generation
    let value_expr = generate_value_desc(field, scry);
    let value_expr = if field.attrs.with.is_none()
        && matches!(field.attrs.fallback, FieldFallback::FromDefaults)
    {
        value_expr
    } else {
        quote! { (#value_expr).without_default_variant() }
    };

    quote! {
        #scry::desc::FieldDesc::new(#field_name, #value_expr)
            .with_doc(#doc)
            #optional_expr
            #default_display
    }
}

fn literal_default_display(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Paren(expr) => literal_default_display(&expr.expr),
        syn::Expr::Group(expr) => literal_default_display(&expr.expr),
        syn::Expr::Lit(syn::ExprLit { lit, .. }) => match lit {
            syn::Lit::Bool(lit) => Some(lit.value.to_string()),
            syn::Lit::Int(lit) => Some(lit.base10_digits().to_string()),
            syn::Lit::Float(lit) => Some(lit.base10_digits().to_string()),
            syn::Lit::Str(lit) => Some(proc_macro2::Literal::string(&lit.value()).to_string()),
            _ => None,
        },
        syn::Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Neg(_),
            expr,
            ..
        }) => numeric_literal_display(expr).map(|display| format!("-{display}")),
        _ => None,
    }
}

fn numeric_literal_display(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Paren(expr) => numeric_literal_display(&expr.expr),
        syn::Expr::Group(expr) => numeric_literal_display(&expr.expr),
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(lit),
            ..
        }) => Some(lit.base10_digits().to_string()),
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Float(lit),
            ..
        }) => Some(lit.base10_digits().to_string()),
        _ => None,
    }
}

fn generate_enum_describe(info: &EnumInfo) -> syn::Result<TokenStream> {
    let scry = scry_crate_path();
    let enum_name = &info.ident;
    let generics = enum_generics(info, Operation::Describe, &scry);
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let doc = doc_override(&info.doc);

    let mut variant_descs = Vec::new();

    for variant in &info.variants {
        let v_ident = &variant.ident;
        let key = variant
            .attrs
            .rename
            .clone()
            .unwrap_or_else(|| rename_all_variant(&v_ident.to_string(), info.attrs.rename_all));
        let variant_doc = &variant.doc;
        let is_default = variant.attrs.is_default;

        match &variant.data {
            VariantData::Unit => {
                variant_descs.push(quote! {
                    #scry::desc::VariantDesc::unit(#key, #is_default)
                        .with_doc(#variant_doc)
                });
            }
            VariantData::Tuple(fields) if fields.len() == 1 => {
                // Single-field tuple: payload is the inner type's desc
                let value = generate_positional_desc(&fields[0], &scry);
                variant_descs.push(quote! {
                    #scry::desc::VariantDesc::payload(
                        #key,
                        #is_default,
                        #value
                    ).with_doc(#variant_doc)
                });
            }
            VariantData::Tuple(fields) => {
                // Multi-field tuple: desc of each element
                let elem_descs: Vec<TokenStream> =
                    fields.iter().map(|field| generate_positional_desc(field, &scry)).collect();
                variant_descs.push(quote! {
                    #scry::desc::VariantDesc::payload(
                        #key,
                        #is_default,
                        #scry::Desc::tuple(vec![#(#elem_descs),*])
                    ).with_doc(#variant_doc)
                });
            }
            VariantData::Struct(fields) => {
                // Struct variant: payload is a struct desc with the nested fields
                let field_descs: Vec<TokenStream> =
                    fields.iter().map(|f| generate_field_desc(f, &scry)).collect();
                variant_descs.push(quote! {
                    #scry::desc::VariantDesc::payload(
                        #key,
                        #is_default,
                        #scry::Desc::structure(vec![#(#field_descs),*])
                    ).with_doc(#variant_doc)
                });
            }
        }
    }

    Ok(quote! {
        impl #impl_generics #scry::Describe for #enum_name #type_generics #where_clause {
            fn describe() -> #scry::Desc {
                #scry::Desc::enumeration(vec![#(#variant_descs),*]) #doc
            }
        }
    })
}

// ---------------------------------------------------------------------------------------------- //
// Shared Field Operations

/// Creates a private binding that cannot shadow a hook's identifiers.
fn private_ident(name: &str) -> Ident {
    Ident::new(&format!("__scry_{name}"), Span::mixed_site())
}

/// Selects complete-field input conversion without changing its container shape.
fn generate_present_field_parser(
    field: &FieldInfo,
    input: &TokenStream,
    scry: &TokenStream,
) -> TokenStream {
    let ty = &field.ty;
    match Operation::FromNode.select(field) {
        FieldOperation::Module(module) => quote! {
            (#module::from_node)(#input)
                .map_err(|error| error.at_path(&(#input).path))
        },
        FieldOperation::Hook(expression) => {
            let call = if is_path_expression(expression) {
                quote! { (#expression)(#input) }
            } else {
                quote! { #scry::_private::call_reader::<#ty>(#input, #expression) }
            };
            quote! { (#call).map_err(|error| error.at_path(&(#input).path)) }
        }
        FieldOperation::Native => quote! { (#input).as_type::<#ty>() },
    }
}

/// Selects complete-field output conversion before the container adds its relative location.
fn generate_field_output(
    field: &FieldInfo,
    value: &TokenStream,
    scry: &TokenStream,
) -> TokenStream {
    let ty = &field.ty;
    match Operation::ToNode.select(field) {
        FieldOperation::Module(module) => {
            quote! { (#module::to_node)(#value) }
        }
        FieldOperation::Hook(expression) if is_path_expression(expression) => {
            quote! { (#expression)(#value) }
        }
        FieldOperation::Hook(expression) => {
            quote! { #scry::_private::call_writer::<#ty>(#value, #expression) }
        }
        FieldOperation::Native => quote! { #scry::ToNode::to_node(#value) },
    }
}

/// Selects the complete value description independently of named-field omission policy.
fn generate_value_desc(field: &FieldInfo, scry: &TokenStream) -> TokenStream {
    let ty = &field.ty;
    match Operation::Describe.select(field) {
        FieldOperation::Module(module) => {
            quote! { (#module::describe)() }
        }
        FieldOperation::Hook(expression) if is_path_expression(expression) => {
            quote! { (#expression)() }
        }
        FieldOperation::Hook(expression) => {
            quote! { #scry::_private::call_describer(#expression) }
        }
        FieldOperation::Native => quote! { <#ty as #scry::Describe>::describe() },
    }
}

/// Preserves ordinary call coercions for paths wrapped in grouping expressions.
fn is_path_expression(expression: &syn::Expr) -> bool {
    match expression {
        syn::Expr::Path(_) => true,
        syn::Expr::Paren(expression) => is_path_expression(&expression.expr),
        syn::Expr::Group(expression) => is_path_expression(&expression.expr),
        _ => false,
    }
}

/// Adds positional field prose without erasing an inherited description when it is absent.
fn generate_positional_desc(field: &FieldInfo, scry: &TokenStream) -> TokenStream {
    let value = generate_value_desc(field, scry);
    let doc = doc_override(&field.doc);
    quote! { (#value) #doc }
}

/// Overrides delegated prose only when the enclosing declaration supplies its own summary.
fn doc_override(doc: &str) -> TokenStream {
    if doc.is_empty() {
        quote! {}
    } else {
        quote! { .with_doc(#doc) }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Generic Requirements

#[derive(Clone, Copy)]
enum Operation {
    FromNode,
    ToNode,
    Describe,
}

impl Operation {
    /// Selects the same field operation for emitted calls and inferred requirements.
    fn select(self, field: &FieldInfo) -> FieldOperation<'_> {
        if let Some(module) = &field.attrs.with {
            return FieldOperation::Module(module);
        }
        let hook = match self {
            Self::FromNode => &field.attrs.from_node_with,
            Self::ToNode => &field.attrs.to_node_with,
            Self::Describe => &field.attrs.describe_with,
        };
        match hook {
            Some(path) => FieldOperation::Hook(path),
            None => FieldOperation::Native,
        }
    }

    fn native_trait(self, scry: &TokenStream) -> TokenStream {
        match self {
            Self::FromNode => quote! { #scry::FromNode },
            Self::ToNode => quote! { #scry::ToNode },
            Self::Describe => quote! { #scry::Describe },
        }
    }
}

#[derive(Clone, Copy)]
enum FieldOperation<'a> {
    Native,
    Hook(&'a syn::Expr),
    Module(&'a syn::Path),
}

fn struct_generics(info: &StructInfo, operation: Operation, scry: &TokenStream) -> Generics {
    let fields = match &info.fields {
        StructFields::Named(fields) | StructFields::Tuple(fields) => fields,
    };
    operation_generics(&info.generics, &info.ident, fields.iter(), operation, scry)
}

fn enum_generics(info: &EnumInfo, operation: Operation, scry: &TokenStream) -> Generics {
    let fields = info.variants.iter().flat_map(|variant| {
        let fields: &[FieldInfo] = match &variant.data {
            VariantData::Unit => &[],
            VariantData::Tuple(fields) | VariantData::Struct(fields) => fields,
        };
        fields.iter()
    });
    operation_generics(&info.generics, &info.ident, fields, operation, scry)
}

/// Adds only the complete-field requirements needed by one generated operation.
fn operation_generics<'a>(
    original: &Generics,
    target: &Ident,
    fields: impl Iterator<Item = &'a FieldInfo>,
    operation: Operation,
    scry: &TokenStream,
) -> Generics {
    let mut generics = original.clone();
    let generic_names: HashSet<String> = original
        .params
        .iter()
        .map(|parameter| match parameter {
            syn::GenericParam::Type(parameter) => parameter.ident.to_string(),
            syn::GenericParam::Const(parameter) => parameter.ident.to_string(),
            syn::GenericParam::Lifetime(parameter) => parameter.lifetime.ident.to_string(),
        })
        .collect();
    let mut existing: HashSet<String> = original
        .where_clause
        .iter()
        .flat_map(|clause| clause.predicates.iter())
        .map(|predicate| predicate.to_token_stream().to_string())
        .collect();

    for field in fields {
        let mut usage = TypeUsage {
            generic_names: &generic_names,
            target,
            generic: false,
            recursive: false,
        };
        usage.visit_type(&field.ty);
        let selected = operation.select(field);
        // A complete recursive-field predicate would require the impl being defined to prove
        // itself. Its body can use the current impl, plus the caller's explicit constraints.
        if !usage.generic || usage.recursive {
            continue;
        }
        let ty = &field.ty;
        let mut predicates: Vec<syn::WherePredicate> = Vec::new();
        match selected {
            FieldOperation::Native => {
                let required = operation.native_trait(scry);
                predicates.push(syn::parse_quote_spanned!(ty.span()=> #ty: #required));
            }
            FieldOperation::Hook(_) | FieldOperation::Module(_) => {}
        }
        if matches!(operation, Operation::FromNode)
            && matches!(field.attrs.fallback, FieldFallback::FromDefaults)
        {
            predicates.push(syn::parse_quote_spanned!(ty.span()=> #ty: #scry::FromDefaults));
        }
        for predicate in predicates {
            if existing.insert(predicate.to_token_stream().to_string()) {
                generics.make_where_clause().predicates.push(predicate);
            }
        }
    }
    generics
}

/// Finds generic dependencies and explicit recursion within a complete field type.
struct TypeUsage<'a> {
    generic_names: &'a HashSet<String>,
    target: &'a Ident,
    generic: bool,
    recursive: bool,
}

impl<'ast> Visit<'ast> for TypeUsage<'_> {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if let Some(first) = path.segments.first() {
            self.generic |= self.generic_names.contains(&first.ident.to_string());
        }
        visit::visit_path(self, path);
    }

    fn visit_type_path(&mut self, ty: &'ast syn::TypePath) {
        if ty.qself.is_none() {
            if let (Some(first), Some(last)) = (ty.path.segments.first(), ty.path.segments.last()) {
                let generic_root = self.generic_names.contains(&first.ident.to_string());
                let local_target = last.ident == *self.target
                    && (ty.path.segments.len() == 1
                        || (ty.path.segments.len() == 2 && first.ident == "self"));
                self.recursive |= !generic_root
                    && ((first.ident == "Self" && ty.path.segments.len() == 1) || local_target);
            }
        }
        visit::visit_type_path(self, ty);
    }

    fn visit_lifetime(&mut self, lifetime: &'ast syn::Lifetime) {
        self.generic |= self.generic_names.contains(&lifetime.ident.to_string());
    }
}

// ---------------------------------------------------------------------------------------------- //

/// Resolves the token path for the `scry` runtime crate.
///
/// Uses `proc-macro-crate` to determine the correct path at compile time:
/// - When invoked from within the `scry` crate itself, produces `crate`.
/// - When invoked from a downstream crate, produces the dependency name
///   (handling renames in Cargo.toml).
fn scry_crate_path() -> TokenStream {
    use proc_macro_crate::{crate_name, FoundCrate};

    let found = crate_name("scry").expect("scry must be present in Cargo.toml");
    match found {
        // Always emit `::scry` (or the renamed ident) rather than `crate`. The scry
        // library crate uses `extern crate self as scry;` so `::scry` resolves
        // everywhere: the library itself, its examples, integration tests, and
        // downstream consumers.
        FoundCrate::Itself => quote! { ::scry },
        FoundCrate::Name(name) => {
            let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
            quote! { ::#ident }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_only_supported_literal_defaults() {
        let cases: Vec<(syn::Expr, Option<&str>)> = vec![
            (syn::parse_quote!(false), Some("false")),
            (syn::parse_quote!(42), Some("42")),
            (syn::parse_quote!(1.25), Some("1.25")),
            (syn::parse_quote!("cache"), Some("\"cache\"")),
            (syn::parse_quote!((-3)), Some("-3")),
            (syn::parse_quote!(0xff_u32), Some("255")),
            (syn::parse_quote!(1_000usize), Some("1000")),
            (syn::parse_quote!(1f32), Some("1")),
            (syn::parse_quote!(r#"cache"#), Some("\"cache\"")),
            (syn::parse_quote!(Vec::new()), None),
            (syn::parse_quote!(PathBuf::from("cache")), None),
            (syn::parse_quote!(OutputMode::Summary), None),
            (syn::parse_quote!(DEFAULT_LIMIT), None),
        ];

        for (expr, expected) in cases {
            assert_eq!(literal_default_display(&expr).as_deref(), expected);
        }
    }

    #[test]
    fn standalone_from_defaults_requires_an_enum_marker() {
        let input: DeriveInput = syn::parse_quote! {
            enum OutputMode {
                Summary,
                Full,
            }
        };

        let error = match derive_from_defaults_impl(&input) {
            Ok(_) => panic!("expected an unmarked enum derive to be rejected"),
            Err(error) => error.to_string(),
        };

        assert!(error.contains("requires exactly one unit variant"));
        assert!(error.contains("#[scry(default)]"));
    }

    #[test]
    fn standalone_from_defaults_rejects_tuple_structs() {
        let input: DeriveInput = syn::parse_quote! {
            struct Wrapper(String);
        };

        let error = match derive_from_defaults_impl(&input) {
            Ok(_) => panic!("expected tuple struct derive to be rejected"),
            Err(error) => error.to_string(),
        };

        assert!(error.contains("only be derived for named structs"));
    }
}
