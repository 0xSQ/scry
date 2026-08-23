//! Code generation for derive macros: FromNode, ToNode, Describe, Config.

use proc_macro2::TokenStream;
use quote::quote;
use syn::DeriveInput;

use crate::parse::{
    self, is_option_type, is_vec_type, rename_all_variant, unwrap_inner_type, unwrap_to_base_type,
    DeriveTarget, EnumInfo, FieldFallback, FieldInfo, StructFields, StructInfo, VariantData,
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
    let from_node = derive_from_node_impl(input)?;
    let desc = derive_describe_impl(input)?;
    let (from_defaults, string_enum) = match parse::parse_input(input)? {
        DeriveTarget::Struct(info) if matches!(&info.fields, StructFields::Named(_)) => {
            (generate_struct_from_defaults(&info)?, quote! {})
        }
        DeriveTarget::Enum(info) => {
            let from_defaults = generate_enum_from_defaults(&info, false)?;
            let string_enum = if info.attrs.from_str {
                generate_enum_string_enum(&info)?
            } else {
                quote! {}
            };
            (from_defaults, string_enum)
        }
        _ => (quote! {}, quote! {}),
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

    Ok(quote! {
        impl #scry::FromDefaults for #struct_name {
            fn from_defaults_at(
                path: &#scry::KeyPath,
            ) -> Result<Self, #scry::NodeError> {
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

    Ok(quote! {
        impl #scry::FromDefaults for #enum_name {
            fn from_defaults_at(
                _path: &#scry::KeyPath,
            ) -> Result<Self, #scry::NodeError> {
                Ok(Self::#variant_name)
            }
        }
    })
}

fn generate_struct_from_node(info: &StructInfo) -> syn::Result<TokenStream> {
    let scry = scry_crate_path();
    let struct_name = &info.ident;

    match &info.fields {
        StructFields::Named(fields) => {
            let field_parsers: Vec<TokenStream> =
                fields.iter().map(generate_field_parser).collect();

            let ensure_no_unknown_keys = if info.allow_unknown_keys {
                quote! {}
            } else {
                quote! { node.ensure_no_unknown_keys()?; }
            };

            Ok(quote! {
                impl #scry::FromNode for #struct_name {
                    fn from_node(node: &#scry::Node) -> Result<Self, #scry::NodeError> {
                        let result = Self {
                            #(#field_parsers),*
                        };
                        #ensure_no_unknown_keys
                        Ok(result)
                    }
                }
            })
        }
        StructFields::Tuple(types) => {
            if types.len() == 1 {
                // Newtype: parse as inner type directly
                Ok(quote! {
                    impl #scry::FromNode for #struct_name {
                        fn from_node(node: &#scry::Node) -> Result<Self, #scry::NodeError> {
                            Ok(Self(#scry::FromNode::from_node(node)?))
                        }
                    }
                })
            } else {
                // Tuple: parse from array
                let field_count = types.len();
                let field_indices: Vec<usize> = (0..field_count).collect();

                Ok(quote! {
                    impl #scry::FromNode for #struct_name {
                        fn from_node(node: &#scry::Node) -> Result<Self, #scry::NodeError> {
                            let arr = node.as_vec()?;
                            if arr.len() != #field_count {
                                return Err(#scry::NodeError::array_length(&node.path, #field_count, arr.len()));
                            }
                            Ok(Self(
                                #(#scry::FromNode::from_node(&arr[#field_indices])?),*
                            ))
                        }
                    }
                })
            }
        }
    }
}

fn generate_field_parser(field: &FieldInfo) -> TokenStream {
    let scry = scry_crate_path();
    let field_name = &field.ident;
    let ty = &field.ty;
    let key = field.attrs.rename.clone().unwrap_or_else(|| field_name.to_string());

    // Custom parse function
    if let Some(ref func_path) = field.attrs.from_node_with {
        match &field.attrs.fallback {
            FieldFallback::Expression(expr) => {
                return quote! {
                    #field_name: match node.opt_node(#key)? {
                        Some(n) => #func_path(n)?,
                        None => #expr,
                    }
                };
            }
            FieldFallback::FromDefaults => {
                return quote! {
                    #field_name: match node.opt_node(#key)? {
                        Some(n) => #func_path(n)?,
                        None => <#ty as #scry::FromDefaults>::from_defaults_at(
                            &node.full_path(#key)?,
                        )?,
                    }
                };
            }
            FieldFallback::Unspecified => {}
        }

        // from_node_with on Option<T>: use opt_node, wrap in Some if present
        if is_option_type(&field.ty) {
            return quote! {
                #field_name: match node.opt_node(#key)? {
                    Some(n) => Some(#func_path(n)?),
                    None => None,
                }
            };
        }

        // Required field with from_node_with
        return quote! {
            #field_name: #func_path(node.req_node(#key)?)?
        };
    }

    match &field.attrs.fallback {
        FieldFallback::Expression(expr) => {
            return quote! {
                #field_name: node.opt(#key)?.unwrap_or_else(|| #expr)
            };
        }
        FieldFallback::FromDefaults => {
            return quote! {
                #field_name: match node.opt_node(#key)? {
                    Some(n) => n.as_type::<#ty>()?,
                    None => <#ty as #scry::FromDefaults>::from_defaults_at(
                        &node.full_path(#key)?,
                    )?,
                }
            };
        }
        FieldFallback::Unspecified => {}
    }

    // Option type (auto-detected) - implicit None default
    if is_option_type(&field.ty) {
        return quote! {
            #field_name: node.opt(#key)?
        };
    }

    // Required field
    quote! {
        #field_name: node.req(#key)?
    }
}

fn generate_enum_from_node(info: &EnumInfo) -> syn::Result<TokenStream> {
    let scry = scry_crate_path();
    let enum_name = &info.ident;

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
    let payload_string_error_arms: Vec<TokenStream> = payload_variants
        .iter()
        .map(|(key, _, _)| {
            let spellings: Vec<String> = parse::variant_spellings(key)
                .into_iter()
                .map(|name| name.to_ascii_lowercase())
                .collect();
            let msg = format!(
                "variant '{}' requires a payload - use {{\"{}\": <value>}} syntax",
                key, key
            );
            quote! {
                #(#spellings)|* => return Err(#scry::NodeError::invalid_value(&node.path, #msg))
            }
        })
        .collect();

    // Generate map match arms for payload variants
    let payload_map_arms: Vec<TokenStream> = payload_variants
        .iter()
        .map(|(key, v_ident, data)| {
            let parse_payload = match data {
                VariantData::Unit => unreachable!(),
                VariantData::Tuple(types) if types.len() == 1 => {
                    // Single-field tuple: payload is the value directly
                    // Parse first, then add helpful hint if it fails on a 1-element array
                    let hint_msg = format!(
                        "hint: payload is a 1-element array - if you meant a single value, \
                        use {{\"{}\": <value>}} instead of {{\"{}\": [<value>]}}",
                        key, key
                    );
                    quote! {
                        match #scry::FromNode::from_node(payload) {
                            Ok(v) => Ok(#enum_name::#v_ident(v)),
                            Err(e) => {
                                // Add hint for common "accidental brackets" mistake
                                if let Some(arr) = payload.as_opt_vec() {
                                    if arr.len() == 1 {
                                        return Err(#scry::NodeError::invalid_value(
                                            &payload.path,
                                            format!("failed to parse variant '{}': {} ({})", #key, e, #hint_msg),
                                        ));
                                    }
                                }
                                Err(e)
                            }
                        }
                    }
                }
                VariantData::Tuple(types) => {
                    // Multi-field tuple: payload is an array
                    let field_count = types.len();
                    let field_parsers: Vec<TokenStream> =
                        (0..field_count).map(|i| quote! { #scry::FromNode::from_node(&arr[#i])? }).collect();

                    quote! {
                        let arr = payload.as_vec()?;
                        if arr.len() != #field_count {
                            return Err(#scry::NodeError::array_length(&payload.path, #field_count, arr.len()));
                        }
                        Ok(#enum_name::#v_ident(#(#field_parsers),*))
                    }
                }
                VariantData::Struct(fields) => {
                    // Struct variant: payload is a map with field semantics
                    let field_parsers: Vec<TokenStream> =
                        fields.iter().map(generate_struct_variant_field_parser).collect();
                    let field_names: Vec<&syn::Ident> = fields.iter().map(|f| &f.ident).collect();

                    quote! {
                        let result = #enum_name::#v_ident {
                            #(#field_names: #field_parsers),*
                        };
                        payload.ensure_no_unknown_keys()?;
                        Ok(result)
                    }
                }
            };

            let spellings = parse::variant_spellings(key);
            quote! { #(#spellings)|* => { #parse_payload } }
        })
        .collect();

    // Generate error arms for unit variants used as map keys
    let unit_map_error_arms: Vec<TokenStream> = unit_variants
        .iter()
        .map(|(key, _)| {
            let spellings = parse::variant_spellings(key);
            let msg = format!(
                "unit variant '{}' must be written as a string \"{}\", not as {{\"{}\":...}}",
                key, key, key
            );
            quote! {
                #(#spellings)|* => return Err(#scry::NodeError::invalid_value(&node.path, #msg))
            }
        })
        .collect();

    Ok(quote! {
        impl #scry::FromNode for #enum_name {
            fn from_node(node: &#scry::Node) -> Result<Self, #scry::NodeError> {
                use #scry::node::Kind;

                match &node.kind {
                    // String form: only unit variants allowed
                    Kind::Leaf(leaf) => {
                        if let #scry::node::Value::String(s) = &leaf.value {
                            match s.to_ascii_lowercase().as_str() {
                                #(#unit_string_arms,)*
                                #(#payload_string_error_arms,)*
                                other => return Err(#scry::NodeError::invalid_value(
                                    &node.path,
                                    format!(
                                        "unknown variant '{}' - expected one of: {}",
                                        other,
                                        #expected_str,
                                    ),
                                )),
                            }
                        } else {
                            return Err(#scry::NodeError::type_mismatch(
                                &node.path,
                                "string or map",
                                leaf.value.type_name(),
                            ));
                        }
                    }

                    // Map form: exactly one key required
                    Kind::Map(map) => {
                        if map.is_empty() {
                            return Err(#scry::NodeError::invalid_value(
                                &node.path,
                                format!("expected exactly one variant key, found empty map - expected one of: {}", #expected_str),
                            ));
                        }
                        if map.len() > 1 {
                            let keys: Vec<&str> = map.keys().map(|s| s.as_str()).collect();
                            return Err(#scry::NodeError::invalid_value(
                                &node.path,
                                format!("expected exactly one variant key, found {}: {}", map.len(), keys.join(", ")),
                            ));
                        }

                        let (variant_key, payload) = map.iter().next().unwrap();
                        match variant_key.as_str() {
                            #(#payload_map_arms,)*
                            #(#unit_map_error_arms,)*
                            other => return Err(#scry::NodeError::invalid_value(
                                &node.path,
                                format!(
                                    "unknown variant '{}' - expected one of: {}",
                                    other,
                                    #expected_str,
                                ),
                            )),
                        }
                    }

                    // Array form: not valid for enums
                    Kind::Vec(_) => {
                        return Err(#scry::NodeError::type_mismatch(
                            &node.path,
                            "string or map",
                            "array",
                        ));
                    }
                }
            }
        }
    })
}

/// Generates field parsing code for a struct variant field.
///
/// Similar to `generate_field_parser` but uses `payload` as the node variable.
fn generate_struct_variant_field_parser(field: &FieldInfo) -> TokenStream {
    let scry = scry_crate_path();
    let ty = &field.ty;
    let key = field.attrs.rename.clone().unwrap_or_else(|| field.ident.to_string());

    // Custom parse function
    if let Some(ref func_path) = field.attrs.from_node_with {
        match &field.attrs.fallback {
            FieldFallback::Expression(expr) => {
                return quote! {
                    match payload.opt_node(#key)? {
                        Some(n) => #func_path(n)?,
                        None => #expr,
                    }
                };
            }
            FieldFallback::FromDefaults => {
                return quote! {
                    match payload.opt_node(#key)? {
                        Some(n) => #func_path(n)?,
                        None => <#ty as #scry::FromDefaults>::from_defaults_at(
                            &payload.full_path(#key)?,
                        )?,
                    }
                };
            }
            FieldFallback::Unspecified => {}
        }

        // from_node_with on Option<T>: use opt_node, wrap in Some if present
        if is_option_type(&field.ty) {
            return quote! {
                match payload.opt_node(#key)? {
                    Some(n) => Some(#func_path(n)?),
                    None => None,
                }
            };
        }

        // Required field with from_node_with
        return quote! { #func_path(payload.req_node(#key)?)? };
    }

    match &field.attrs.fallback {
        FieldFallback::Expression(expr) => {
            return quote! { payload.opt(#key)?.unwrap_or_else(|| #expr) };
        }
        FieldFallback::FromDefaults => {
            return quote! {
                match payload.opt_node(#key)? {
                    Some(n) => n.as_type::<#ty>()?,
                    None => <#ty as #scry::FromDefaults>::from_defaults_at(
                        &payload.full_path(#key)?,
                    )?,
                }
            };
        }
        FieldFallback::Unspecified => {}
    }

    // Option type (auto-detected) - implicit None default
    if is_option_type(&field.ty) {
        return quote! { payload.opt(#key)? };
    }

    // Required field
    quote! { payload.req(#key)? }
}

// ---------------------------------------------------------------------------------------------- //
// ToNode Generation

fn generate_struct_to_node(info: &StructInfo) -> syn::Result<TokenStream> {
    let scry = scry_crate_path();
    let struct_name = &info.ident;

    match &info.fields {
        StructFields::Named(fields) => {
            let field_serializers: Vec<TokenStream> =
                fields.iter().map(|f| generate_field_serializer(f, &scry)).collect();

            Ok(quote! {
                impl #scry::ToNode for #struct_name {
                    fn to_node(&self) -> Result<#scry::Node, #scry::NodeError> {
                        let mut map = #scry::_private::IndexMap::new();
                        #(#field_serializers)*
                        Ok(#scry::Node {
                            path: #scry::KeyPath::new(),
                            kind: #scry::node::Kind::Map(map),
                        })
                    }
                }
            })
        }
        StructFields::Tuple(types) => {
            if types.len() == 1 {
                // Newtype: delegate to inner
                Ok(quote! {
                    impl #scry::ToNode for #struct_name {
                        fn to_node(&self) -> Result<#scry::Node, #scry::NodeError> {
                            #scry::ToNode::to_node(&self.0)
                        }
                    }
                })
            } else {
                // Tuple: serialize as array
                let field_indices: Vec<syn::Index> =
                    (0..types.len()).map(syn::Index::from).collect();

                Ok(quote! {
                    impl #scry::ToNode for #struct_name {
                        fn to_node(&self) -> Result<#scry::Node, #scry::NodeError> {
                            let children = vec![
                                #(#scry::ToNode::to_node(&self.#field_indices)?),*
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

fn generate_field_serializer(field: &FieldInfo, scry: &TokenStream) -> TokenStream {
    let field_name = &field.ident;
    let key = field.attrs.rename.clone().unwrap_or_else(|| field_name.to_string());

    // Custom to_node_with function
    if let Some(ref func_path) = field.attrs.to_node_with {
        if is_option_type(&field.ty) {
            return quote! {
                if let Some(ref value) = self.#field_name {
                    map.insert(#key.to_string(), #func_path(value)?);
                }
            };
        } else {
            return quote! {
                map.insert(#key.to_string(), #func_path(&self.#field_name)?);
            };
        }
    }

    // For Option<T> fields, only serialize if Some (omit None)
    if is_option_type(&field.ty) {
        quote! {
            if let Some(ref value) = self.#field_name {
                map.insert(#key.to_string(), #scry::ToNode::to_node(value)?);
            }
        }
    } else {
        quote! {
            map.insert(#key.to_string(), #scry::ToNode::to_node(&self.#field_name)?);
        }
    }
}

/// Generates field serialization code for struct variant fields.
///
/// Similar to `generate_field_serializer` but uses bound variable names
/// and inserts into `inner_map` instead of `map`.
fn generate_struct_variant_field_serializer(field: &FieldInfo, scry: &TokenStream) -> TokenStream {
    let field_name = &field.ident;
    let key = field.attrs.rename.clone().unwrap_or_else(|| field_name.to_string());

    // Custom to_node_with function
    if let Some(ref func_path) = field.attrs.to_node_with {
        if is_option_type(&field.ty) {
            return quote! {
                if let Some(ref value) = #field_name {
                    inner_map.insert(#key.to_string(), #func_path(value)?);
                }
            };
        } else {
            return quote! {
                inner_map.insert(#key.to_string(), #func_path(#field_name)?);
            };
        }
    }

    // For Option<T> fields, only serialize if Some (omit None)
    if is_option_type(&field.ty) {
        quote! {
            if let Some(ref value) = #field_name {
                inner_map.insert(#key.to_string(), #scry::ToNode::to_node(value)?);
            }
        }
    } else {
        quote! {
            inner_map.insert(#key.to_string(), #scry::ToNode::to_node(#field_name)?);
        }
    }
}

fn generate_enum_to_node(info: &EnumInfo) -> syn::Result<TokenStream> {
    let scry = scry_crate_path();
    let enum_name = &info.ident;

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
            VariantData::Tuple(types) if types.len() == 1 => {
                // Single-field tuple: serialize as {"name": value}
                match_arms.push(quote! {
                    #enum_name::#v_ident(inner) => {
                        let mut map = #scry::_private::IndexMap::new();
                        map.insert(#key.to_string(), #scry::ToNode::to_node(inner)?);
                        Ok(#scry::Node {
                            path: #scry::KeyPath::new(),
                            kind: #scry::node::Kind::Map(map),
                        })
                    }
                });
            }
            VariantData::Tuple(types) => {
                // Multi-field tuple: serialize as {"name": [v1, v2, ...]}
                let field_count = types.len();
                let field_bindings: Vec<syn::Ident> = (0..field_count)
                    .map(|i| syn::Ident::new(&format!("f{}", i), proc_macro2::Span::call_site()))
                    .collect();
                let field_serializers: Vec<TokenStream> = field_bindings
                    .iter()
                    .map(|binding| quote! { #scry::ToNode::to_node(#binding)? })
                    .collect();

                match_arms.push(quote! {
                    #enum_name::#v_ident(#(#field_bindings),*) => {
                        let mut map = #scry::_private::IndexMap::new();
                        let inner_vec = vec![#(#field_serializers),*];
                        map.insert(#key.to_string(), #scry::Node {
                            path: #scry::KeyPath::new(),
                            kind: #scry::node::Kind::Vec(inner_vec),
                        });
                        Ok(#scry::Node {
                            path: #scry::KeyPath::new(),
                            kind: #scry::node::Kind::Map(map),
                        })
                    }
                });
            }
            VariantData::Struct(fields) => {
                // Struct variant: serialize as {"name": {"f1": v1, "f2": v2}}
                // Respects rename and omits None for Option<T> fields
                let field_names: Vec<&syn::Ident> = fields.iter().map(|f| &f.ident).collect();
                let field_serializers: Vec<TokenStream> = fields
                    .iter()
                    .map(|f| generate_struct_variant_field_serializer(f, &scry))
                    .collect();

                match_arms.push(quote! {
                    #enum_name::#v_ident { #(#field_names),* } => {
                        let mut inner_map = #scry::_private::IndexMap::new();
                        #(#field_serializers)*
                        let mut map = #scry::_private::IndexMap::new();
                        map.insert(#key.to_string(), #scry::Node {
                            path: #scry::KeyPath::new(),
                            kind: #scry::node::Kind::Map(inner_map),
                        });
                        Ok(#scry::Node {
                            path: #scry::KeyPath::new(),
                            kind: #scry::node::Kind::Map(map),
                        })
                    }
                });
            }
        }
    }

    Ok(quote! {
        impl #scry::ToNode for #enum_name {
            fn to_node(&self) -> Result<#scry::Node, #scry::NodeError> {
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
        impl std::str::FromStr for #enum_name {
            type Err = #scry::StringEnumError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s.to_ascii_lowercase().as_str() {
                    #(#parse_arms,)*
                    _ => Err(#scry::StringEnumError::new(stringify!(#enum_name), s, #expected_str)),
                }
            }
        }

        impl std::fmt::Display for #enum_name {
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

    match &info.fields {
        StructFields::Named(fields) => {
            let field_descs: Vec<TokenStream> =
                fields.iter().map(|f| generate_field_desc(f, &scry)).collect();

            Ok(quote! {
                impl #scry::Describe for #struct_name {
                    fn describe() -> #scry::Desc {
                        #scry::Desc::structure(vec![
                            #(#field_descs),*
                        ])
                    }
                }
            })
        }
        StructFields::Tuple(types) => {
            if types.len() == 1 {
                // Newtype: desc of the inner type
                let ty = &types[0];
                Ok(quote! {
                    impl #scry::Describe for #struct_name {
                        fn describe() -> #scry::Desc {
                            use #scry::DescFallback;
                            #scry::make_desc_probe::<#ty>().describe().unwrap_or_else(#scry::Desc::default)
                        }
                    }
                })
            } else {
                // Tuple: desc of each element
                let elem_descs: Vec<TokenStream> = types
                    .iter()
                    .map(|ty| {
                        quote! {
                            {
                                use #scry::DescFallback;
                                #scry::make_desc_probe::<#ty>().describe().unwrap_or_else(#scry::Desc::default)
                            }
                        }
                    })
                    .collect();
                Ok(quote! {
                    impl #scry::Describe for #struct_name {
                        fn describe() -> #scry::Desc {
                            #scry::Desc::tuple(vec![#(#elem_descs),*])
                        }
                    }
                })
            }
        }
    }
}

fn generate_field_desc(field: &FieldInfo, scry: &TokenStream) -> TokenStream {
    let field_name = field.attrs.rename.clone().unwrap_or_else(|| field.ident.to_string());
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
    let value_expr = if let Some(ref desc_fn) = field.attrs.describe_with {
        quote! { #desc_fn() }
    } else {
        // Unwrap Option<T> to get the inner type for desc generation
        let desc_ty = if is_option_type(ty) {
            unwrap_inner_type(ty).unwrap_or(ty).clone()
        } else {
            ty.clone()
        };

        // Check if this is a Vec type
        let is_list = is_vec_type(&desc_ty);

        // Get the base type for desc lookup (fully unwrapped)
        let base_ty = unwrap_to_base_type(&desc_ty);

        // Generate the value Desc
        if is_list {
            quote! {
                {
                    use #scry::DescFallback;
                    let inner = #scry::make_desc_probe::<#base_ty>().describe().unwrap_or_else(#scry::Desc::default);
                    #scry::Desc::list(inner)
                }
            }
        } else {
            quote! {
                {
                    use #scry::DescFallback;
                    #scry::make_desc_probe::<#base_ty>().describe().unwrap_or_else(#scry::Desc::default)
                }
            }
        }
    };
    let value_expr = if matches!(field.attrs.fallback, FieldFallback::FromDefaults) {
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
            VariantData::Tuple(types) if types.len() == 1 => {
                // Single-field tuple: payload is the inner type's desc
                let ty = &types[0];
                variant_descs.push(quote! {
                    #scry::desc::VariantDesc::payload(
                        #key,
                        #is_default,
                        {
                            use #scry::DescFallback;
                            #scry::make_desc_probe::<#ty>().describe().unwrap_or_else(#scry::Desc::default)
                        }
                    ).with_doc(#variant_doc)
                });
            }
            VariantData::Tuple(types) => {
                // Multi-field tuple: desc of each element
                let elem_descs: Vec<TokenStream> = types
                    .iter()
                    .map(|ty| {
                        quote! {
                            {
                                use #scry::DescFallback;
                                #scry::make_desc_probe::<#ty>().describe().unwrap_or_else(#scry::Desc::default)
                            }
                        }
                    })
                    .collect();
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
        impl #scry::Describe for #enum_name {
            fn describe() -> #scry::Desc {
                #scry::Desc::enumeration(vec![#(#variant_descs),*])
            }
        }
    })
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
