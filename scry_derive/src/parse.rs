use heck::{ToKebabCase, ToSnakeCase};
use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::{Attribute, DeriveInput, Expr, Fields, Ident, LitStr, Result, Type};

// ---------------------------------------------------------------------------------------------- //
// Field Attributes

#[derive(Default)]
pub struct FieldAttrs {
    pub fallback: FieldFallback,
    fallback_span: Option<Span>,
    pub rename: Option<String>,
    /// Custom Node → T conversion function. Set by `from_node_with(...)`.
    pub from_node_with: Option<syn::Path>,
    /// Custom description function. Set by `describe_with(...)`.
    pub describe_with: Option<syn::Path>,
    /// Custom T → Node conversion function. Set by `to_node_with(...)`.
    pub to_node_with: Option<syn::Path>,
}

#[derive(Default)]
pub enum FieldFallback {
    #[default]
    Unspecified,
    Expression(Expr),
    FromDefaults,
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
                        let expr: Expr = meta.input.parse()?;
                        result.set_fallback(FieldFallback::Expression(expr), meta.path.span())?;
                    } else {
                        return Err(meta.error(
                            "bare `default` is ambiguous; use `default = EXPR` for an explicit \
                             value or `from_defaults` for recursive Scry defaults",
                        ));
                    }
                    Ok(())
                } else if meta.path.is_ident("from_defaults") {
                    result.set_fallback(FieldFallback::FromDefaults, meta.path.span())?;
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

    pub fn validate_for_type(&self, ty: &Type) -> Result<()> {
        if matches!(self.fallback, FieldFallback::FromDefaults) && is_option_type(ty) {
            return Err(syn::Error::new(
                self.fallback_span.unwrap_or_else(Span::call_site),
                "`from_defaults` is not supported on `Option<T>` fields; remove the attribute \
                 to use the implicit `None` fallback",
            ));
        }

        Ok(())
    }

    fn set_fallback(&mut self, fallback: FieldFallback, span: Span) -> Result<()> {
        if !matches!(self.fallback, FieldFallback::Unspecified) {
            let message = match (&self.fallback, &fallback) {
                (FieldFallback::Expression(_), FieldFallback::Expression(_)) => {
                    "duplicate `default = EXPR` field fallback"
                }
                (FieldFallback::FromDefaults, FieldFallback::FromDefaults) => {
                    "duplicate `from_defaults` field fallback"
                }
                _ => {
                    "conflicting Scry field fallbacks; use only one of `default = EXPR` or \
                     `from_defaults`"
                }
            };
            return Err(syn::Error::new(span, message));
        }

        self.fallback = fallback;
        self.fallback_span = Some(span);
        Ok(())
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
    default_span: Option<Span>,
}

impl VariantAttrs {
    pub fn from_attrs(attrs: &[Attribute]) -> Result<Self> {
        let mut result = VariantAttrs::default();

        for attr in attrs {
            if !attr.path().is_ident("scry") {
                continue;
            }

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    let _: syn::Token![=] = meta.input.parse()?;
                    let lit: LitStr = meta.input.parse()?;
                    result.rename = Some(lit.value());
                    Ok(())
                } else if meta.path.is_ident("default") {
                    if meta.input.peek(syn::Token![=]) {
                        return Err(meta.error(
                            "`default` on an enum variant is a marker; write `#[scry(default)]`",
                        ));
                    }
                    if result.is_default {
                        return Err(meta.error("duplicate `#[scry(default)]` marker"));
                    }
                    result.is_default = true;
                    result.default_span = Some(meta.path.span());
                    Ok(())
                } else {
                    Err(meta.error("unknown scry variant attribute"))
                }
            })?;
        }

        Ok(result)
    }

    fn default_span(&self) -> Span {
        self.default_span.unwrap_or_else(Span::call_site)
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
                field_infos.push(parse_field(field)?);
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
    let mut default_span = None;

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
                    field_infos.push(parse_field(f)?);
                }
                VariantData::Struct(field_infos)
            }
        };

        let attrs = VariantAttrs::from_attrs(&variant.attrs)?;
        if attrs.is_default {
            if !matches!(data, VariantData::Unit) {
                return Err(syn::Error::new(
                    attrs.default_span(),
                    "`#[scry(default)]` is only supported on unit enum variants",
                ));
            }
            if default_span.is_some() {
                return Err(syn::Error::new(
                    attrs.default_span(),
                    "multiple `#[scry(default)]` enum variants; mark exactly one unit variant",
                ));
            }
            default_span = Some(attrs.default_span());
        }

        variants.push(VariantInfo {
            ident: variant.ident.clone(),
            attrs,
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

fn parse_field(field: &syn::Field) -> Result<FieldInfo> {
    let attrs = FieldAttrs::from_attrs(&field.attrs)?;
    attrs.validate_for_type(&field.ty)?;

    Ok(FieldInfo {
        ident: field.ident.clone().expect("named fields have identifiers"),
        ty: field.ty.clone(),
        attrs,
        doc: extract_doc(&field.attrs),
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

/// Extracts the summary of a doc comment from attributes.
///
/// Only the first paragraph - the lines up to the first blank doc line - is kept, following
/// rustdoc's convention: the first paragraph is the short description, and everything after it
/// is elaboration for source readers that would bloat `--desc` output and error field listings.
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

    docs.split(|line| line.is_empty())
        .find(|paragraph| !paragraph.is_empty())
        .unwrap_or_default()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_expression_is_a_field_fallback() {
        let field: syn::Field = syn::parse_quote! {
            #[scry(default = Vec::new())]
            values: Vec<String>
        };

        let attrs = FieldAttrs::from_attrs(&field.attrs).unwrap();

        assert!(matches!(attrs.fallback, FieldFallback::Expression(_)));
    }

    #[test]
    fn from_defaults_is_a_field_fallback() {
        let field: syn::Field = syn::parse_quote! {
            #[scry(from_defaults)]
            child: Child
        };

        let attrs = FieldAttrs::from_attrs(&field.attrs).unwrap();

        assert!(matches!(attrs.fallback, FieldFallback::FromDefaults));
    }

    #[test]
    fn bare_default_explains_both_replacements() {
        let field: syn::Field = syn::parse_quote! {
            #[scry(default)]
            child: Child
        };

        let error = field_attrs_error(&field);

        assert!(error.contains("default = EXPR"));
        assert!(error.contains("from_defaults"));
    }

    #[test]
    fn conflicting_fallbacks_are_rejected() {
        let field: syn::Field = syn::parse_quote! {
            #[scry(default = Child::new(), from_defaults)]
            child: Child
        };

        let error = field_attrs_error(&field);

        assert!(error.contains("conflicting Scry field fallbacks"));
    }

    #[test]
    fn duplicate_fallbacks_are_rejected() {
        let field: syn::Field = syn::parse_quote! {
            #[scry(from_defaults, from_defaults)]
            child: Child
        };

        let error = field_attrs_error(&field);

        assert!(error.contains("duplicate `from_defaults` field fallback"));
    }

    #[test]
    fn from_defaults_on_option_is_rejected() {
        let input: DeriveInput = syn::parse_quote! {
            struct Parent {
                #[scry(from_defaults)]
                child: Option<Child>,
            }
        };

        let error = match parse_input(&input) {
            Ok(_) => panic!("expected `from_defaults` on `Option<T>` to be rejected"),
            Err(error) => error.to_string(),
        };

        assert!(error.contains("not supported on `Option<T>` fields"));
        assert!(error.contains("implicit `None` fallback"));
    }

    #[test]
    fn enum_accepts_one_scry_default_unit_variant() {
        let input: DeriveInput = syn::parse_quote! {
            enum OutputMode {
                #[scry(default)]
                Summary,
                Full,
            }
        };

        let DeriveTarget::Enum(info) = parse_input(&input).unwrap() else {
            panic!("expected an enum");
        };

        assert!(info.variants[0].attrs.is_default);
        assert!(!info.variants[1].attrs.is_default);
    }

    #[test]
    fn enum_rejects_multiple_scry_default_variants() {
        let input: DeriveInput = syn::parse_quote! {
            enum OutputMode {
                #[scry(default)]
                Summary,
                #[scry(default)]
                Full,
            }
        };

        let error = parse_input_error(&input);

        assert!(error.contains("multiple `#[scry(default)]` enum variants"));
        assert!(error.contains("exactly one unit variant"));
    }

    #[test]
    fn enum_rejects_a_duplicate_marker_on_one_variant() {
        let input: DeriveInput = syn::parse_quote! {
            enum OutputMode {
                #[scry(default, default)]
                Summary,
            }
        };

        let error = parse_input_error(&input);

        assert!(error.contains("duplicate `#[scry(default)]` marker"));
    }

    #[test]
    fn enum_rejects_a_default_payload_variant() {
        let input: DeriveInput = syn::parse_quote! {
            enum OutputMode {
                #[scry(default)]
                Custom(String),
            }
        };

        let error = parse_input_error(&input);

        assert!(error.contains("only supported on unit enum variants"));
    }

    #[test]
    fn rust_default_marker_is_unrelated() {
        let input: DeriveInput = syn::parse_quote! {
            enum OutputMode {
                #[default]
                Summary,
                Full,
            }
        };

        let DeriveTarget::Enum(info) = parse_input(&input).unwrap() else {
            panic!("expected an enum");
        };

        assert!(info.variants.iter().all(|variant| !variant.attrs.is_default));
    }

    fn field_attrs_error(field: &syn::Field) -> String {
        match FieldAttrs::from_attrs(&field.attrs) {
            Ok(_) => panic!("expected field attributes to be rejected"),
            Err(error) => error.to_string(),
        }
    }

    fn parse_input_error(input: &DeriveInput) -> String {
        match parse_input(input) {
            Ok(_) => panic!("expected derive input to be rejected"),
            Err(error) => error.to_string(),
        }
    }
}
