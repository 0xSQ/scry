use heck::{ToKebabCase, ToSnakeCase};
use syn::{Attribute, DeriveInput, Fields, Ident, LitStr, Result, Type};

// ---------------------------------------------------------------------------------------------- //
// Field Attributes

#[derive(Default)]
pub struct FieldAttrs {
    pub default: bool,
    pub default_value: Option<syn::Expr>,
    pub rename: Option<String>,
    /// Custom Node → T conversion function. Set by `from_node_with(...)`.
    pub from_node_with: Option<syn::Path>,
    /// Custom description function. Set by `describe_with(...)`.
    pub describe_with: Option<syn::Path>,
    /// Custom T → Node conversion function. Set by `to_node_with(...)`.
    pub to_node_with: Option<syn::Path>,
}

impl FieldAttrs {
    /// Checks if any `#[scry(...)]` attribute is present.
    pub fn has_scry_attr(attrs: &[Attribute]) -> bool {
        attrs.iter().any(|attr| attr.path().is_ident("scry"))
    }

    pub fn from_attrs(attrs: &[Attribute]) -> Result<Self> {
        let mut result = FieldAttrs::default();

        for attr in attrs {
            if !attr.path().is_ident("scry") {
                continue;
            }

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("default") {
                    if meta.input.peek(syn::Token![=]) {
                        let _: syn::Token![=] = meta.input.parse()?;
                        let expr: syn::Expr = meta.input.parse()?;
                        result.default_value = Some(expr);
                    } else {
                        result.default = true;
                    }
                    Ok(())
                } else if meta.path.is_ident("rename") {
                    let _: syn::Token![=] = meta.input.parse()?;
                    let lit: LitStr = meta.input.parse()?;
                    result.rename = Some(lit.value());
                    Ok(())
                } else if meta.path.is_ident("from_node_with") {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let path: syn::Path = content.parse()?;
                    result.from_node_with = Some(path);
                    Ok(())
                } else if meta.path.is_ident("describe_with") {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let path: syn::Path = content.parse()?;
                    result.describe_with = Some(path);
                    Ok(())
                } else if meta.path.is_ident("to_node_with") {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let path: syn::Path = content.parse()?;
                    result.to_node_with = Some(path);
                    Ok(())
                } else {
                    Err(meta.error("unknown scry field attribute"))
                }
            })?;
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------------------------- //
// Struct Attributes

#[derive(Default)]
pub struct StructAttrs {
    pub allow_unknown_keys: bool,
}

impl StructAttrs {
    pub fn from_attrs(attrs: &[Attribute]) -> Result<Self> {
        let mut result = StructAttrs::default();

        for attr in attrs {
            if !attr.path().is_ident("scry") {
                continue;
            }

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("allow_unknown_keys") {
                    result.allow_unknown_keys = true;
                    Ok(())
                } else {
                    Err(meta.error("unknown scry struct attribute"))
                }
            })?;
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------------------------- //
// Variant Attributes

#[derive(Default)]
pub struct VariantAttrs {
    pub rename: Option<String>,
    pub is_default: bool,
}

impl VariantAttrs {
    pub fn from_attrs(attrs: &[Attribute]) -> Result<Self> {
        let mut result = VariantAttrs::default();

        for attr in attrs {
            // Check for #[default] attribute (standard Rust derive helper)
            if attr.path().is_ident("default") {
                result.is_default = true;
                continue;
            }

            if !attr.path().is_ident("scry") {
                continue;
            }

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    let _: syn::Token![=] = meta.input.parse()?;
                    let lit: LitStr = meta.input.parse()?;
                    result.rename = Some(lit.value());
                    Ok(())
                } else {
                    Err(meta.error("unknown scry variant attribute"))
                }
            })?;
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------------------------- //
// Enum Attributes

#[derive(Clone, Copy)]
pub enum RenameAll {
    SnakeCase,
    KebabCase,
}

#[derive(Default)]
pub struct EnumAttrs {
    pub rename_all: Option<RenameAll>,
    pub from_str: bool,
}

impl EnumAttrs {
    pub fn from_attrs(attrs: &[Attribute]) -> Result<Self> {
        let mut result = EnumAttrs::default();

        for attr in attrs {
            if !attr.path().is_ident("scry") {
                continue;
            }

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename_all") {
                    let _: syn::Token![=] = meta.input.parse()?;
                    let lit: LitStr = meta.input.parse()?;
                    result.rename_all = Some(parse_rename_all(&lit)?);
                    Ok(())
                } else if meta.path.is_ident("from_str") {
                    result.from_str = true;
                    Ok(())
                } else {
                    Err(meta.error("unknown scry enum attribute"))
                }
            })?;
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------------------------- //
// Struct Info

pub struct FieldInfo {
    pub ident: Ident,
    pub ty: Type,
    pub attrs: FieldAttrs,
    pub doc: String,
}

pub enum StructFields {
    Named(Vec<FieldInfo>),
    Tuple(Vec<Type>),
}

pub struct StructInfo {
    pub ident: Ident,
    pub fields: StructFields,
    pub allow_unknown_keys: bool,
}

// ---------------------------------------------------------------------------------------------- //
// Enum Info

pub struct VariantInfo {
    pub ident: Ident,
    pub attrs: VariantAttrs,
    pub data: VariantData,
    pub doc: String,
}

pub enum VariantData {
    Unit,
    Tuple(Vec<Type>),
    Struct(Vec<FieldInfo>),
}

pub struct EnumInfo {
    pub ident: Ident,
    pub attrs: EnumAttrs,
    pub variants: Vec<VariantInfo>,
}

// ---------------------------------------------------------------------------------------------- //
// Top-level Parse Target

pub enum DeriveTarget {
    Struct(StructInfo),
    Enum(EnumInfo),
}

// ---------------------------------------------------------------------------------------------- //
// Parsing Functions

pub fn parse_input(input: &DeriveInput) -> Result<DeriveTarget> {
    match &input.data {
        syn::Data::Struct(data) => Ok(DeriveTarget::Struct(parse_struct(input, data)?)),
        syn::Data::Enum(data) => Ok(DeriveTarget::Enum(parse_enum(input, data)?)),
        syn::Data::Union(_) => {
            Err(syn::Error::new_spanned(input, "Scry cannot be derived for unions"))
        }
    }
}

fn parse_struct(input: &DeriveInput, data: &syn::DataStruct) -> Result<StructInfo> {
    let struct_attrs = StructAttrs::from_attrs(&input.attrs)?;

    let fields = match &data.fields {
        Fields::Named(fields) => {
            let mut field_infos = Vec::new();
            for field in &fields.named {
                field_infos.push(FieldInfo {
                    ident: field.ident.clone().unwrap(),
                    ty: field.ty.clone(),
                    attrs: FieldAttrs::from_attrs(&field.attrs)?,
                    doc: extract_doc(&field.attrs),
                });
            }
            StructFields::Named(field_infos)
        }
        Fields::Unnamed(fields) => {
            // Check for #[scry(...)] on unnamed fields - not supported
            for field in &fields.unnamed {
                if FieldAttrs::has_scry_attr(&field.attrs) {
                    return Err(syn::Error::new_spanned(
                        field,
                        "#[scry(...)] attributes are not supported on tuple struct fields",
                    ));
                }
            }
            let types: Vec<Type> = fields.unnamed.iter().map(|f| f.ty.clone()).collect();
            StructFields::Tuple(types)
        }
        Fields::Unit => {
            return Err(syn::Error::new_spanned(input, "Scry cannot be derived for unit structs"))
        }
    };

    Ok(StructInfo {
        ident: input.ident.clone(),
        fields,
        allow_unknown_keys: struct_attrs.allow_unknown_keys,
    })
}

fn parse_enum(input: &DeriveInput, data: &syn::DataEnum) -> Result<EnumInfo> {
    let enum_attrs = EnumAttrs::from_attrs(&input.attrs)?;
    let mut variants = Vec::new();

    for variant in &data.variants {
        let data = match &variant.fields {
            Fields::Unit => VariantData::Unit,
            Fields::Unnamed(fields) => {
                // Check for #[scry(...)] on unnamed fields - not supported
                for field in &fields.unnamed {
                    if FieldAttrs::has_scry_attr(&field.attrs) {
                        return Err(syn::Error::new_spanned(
                            field,
                            "#[scry(...)] attributes are not supported on tuple variant fields",
                        ));
                    }
                }
                let types: Vec<Type> = fields.unnamed.iter().map(|f| f.ty.clone()).collect();
                VariantData::Tuple(types)
            }
            Fields::Named(fields) => {
                let mut field_infos = Vec::new();
                for f in &fields.named {
                    field_infos.push(FieldInfo {
                        ident: f.ident.clone().unwrap(),
                        ty: f.ty.clone(),
                        attrs: FieldAttrs::from_attrs(&f.attrs)?,
                        doc: extract_doc(&f.attrs),
                    });
                }
                VariantData::Struct(field_infos)
            }
        };

        variants.push(VariantInfo {
            ident: variant.ident.clone(),
            attrs: VariantAttrs::from_attrs(&variant.attrs)?,
            data,
            doc: extract_doc(&variant.attrs),
        });
    }

    Ok(EnumInfo {
        ident: input.ident.clone(),
        attrs: enum_attrs,
        variants,
    })
}

// ---------------------------------------------------------------------------------------------- //
// Type Utilities

/// Checks if the type is Option<T>.
pub fn is_option_type(ty: &Type) -> bool {
    get_wrapper_kind(ty) == Some("Option")
}

/// Checks if the type is Vec<T>.
pub fn is_vec_type(ty: &Type) -> bool {
    get_wrapper_kind(ty) == Some("Vec")
}

/// Returns the wrapper kind if the type is Option, Vec, or Arc.
pub fn get_wrapper_kind(ty: &Type) -> Option<&'static str> {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            return match segment.ident.to_string().as_str() {
                "Option" => Some("Option"),
                "Vec" => Some("Vec"),
                "Arc" => Some("Arc"),
                _ => None,
            };
        }
    }
    None
}

/// Extracts inner type from Option<T>, Vec<T>, or Arc<T>.
///
/// Returns None if not a wrapper type.
pub fn unwrap_inner_type(ty: &Type) -> Option<&Type> {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            if segment.ident == "Option" || segment.ident == "Vec" || segment.ident == "Arc" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        return Some(inner);
                    }
                }
            }
        }
    }
    None
}

/// Recursively unwraps through Arc, Option, Vec to get the base type.
pub fn unwrap_to_base_type(ty: &Type) -> &Type {
    if let Some(inner) = unwrap_inner_type(ty) {
        unwrap_to_base_type(inner)
    } else {
        ty
    }
}

// ---------------------------------------------------------------------------------------------- //
// String Utilities

pub fn rename_all_variant(s: &str, rename_all: Option<RenameAll>) -> String {
    match rename_all.unwrap_or(RenameAll::SnakeCase) {
        RenameAll::SnakeCase => s.to_snake_case(),
        RenameAll::KebabCase => s.to_kebab_case(),
    }
}

pub fn variant_spellings(s: &str) -> Vec<String> {
    let mut spellings = vec![s.to_string()];

    if s.contains('_') || s.contains('-') {
        let snake = s.replace('-', "_");
        let kebab = s.replace('_', "-");

        if !spellings.contains(&snake) {
            spellings.push(snake);
        }
        if !spellings.contains(&kebab) {
            spellings.push(kebab);
        }
    }

    spellings
}

fn parse_rename_all(lit: &LitStr) -> Result<RenameAll> {
    match lit.value().as_str() {
        "snake_case" | "snake-case" => Ok(RenameAll::SnakeCase),
        "kebab-case" | "kebab_case" => Ok(RenameAll::KebabCase),
        other => {
            Err(syn::Error::new_spanned(lit, format!("unsupported rename_all value '{other}'")))
        }
    }
}

/// Extracts doc comments from attributes.
pub fn extract_doc(attrs: &[Attribute]) -> String {
    let docs: Vec<String> = attrs
        .iter()
        .filter_map(|attr| {
            if attr.path().is_ident("doc") {
                if let syn::Meta::NameValue(nv) = &attr.meta {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) = &nv.value
                    {
                        return Some(s.value().trim().to_string());
                    }
                }
            }
            None
        })
        .collect();

    docs.join(" ")
}
