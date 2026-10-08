use std::collections::HashMap;

use heck::{ToKebabCase, ToSnakeCase};
use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::{Attribute, DeriveInput, Expr, Fields, Generics, Ident, LitStr, Member, Result, Type};

// ---------------------------------------------------------------------------------------------- //

pub enum DeriveTarget {
    Struct(StructInfo),
    Enum(EnumInfo),
}

pub struct StructInfo {
    pub ident: Ident,
    pub generics: Generics,
    pub fields: StructFields,
    pub allow_unknown_keys: bool,
    pub doc: String,
}

pub enum StructFields {
    Named(Vec<FieldInfo>),
    Tuple(Vec<FieldInfo>),
}

pub struct EnumInfo {
    pub ident: Ident,
    pub generics: Generics,
    pub attrs: EnumAttrs,
    pub variants: Vec<VariantInfo>,
    pub doc: String,
}

pub struct VariantInfo {
    pub ident: Ident,
    pub attrs: VariantAttrs,
    pub data: VariantData,
    pub doc: String,
}

pub enum VariantData {
    Unit,
    Tuple(Vec<FieldInfo>),
    Struct(Vec<FieldInfo>),
}

pub struct FieldInfo {
    pub member: Member,
    pub ty: Type,
    pub attrs: FieldAttrs,
    pub doc: String,
}

impl FieldInfo {
    /// Returns the literal configuration key of a named field.
    pub fn config_key(&self) -> String {
        let Member::Named(ident) = &self.member else {
            unreachable!("positional fields do not have configuration keys");
        };
        self.attrs.rename.clone().unwrap_or_else(|| ident.to_string())
    }
}

// ---------------------------------------------------------------------------------------------- //
// Field Attributes

#[derive(Default)]
pub struct FieldAttrs {
    pub fallback: FieldFallback,
    fallback_span: Option<Span>,
    pub rename: Option<String>,
    rename_span: Option<Span>,
    /// Module supplying the requested operations for the complete field value.
    pub with: Option<syn::Path>,
    with_span: Option<Span>,
    /// Callable Node-to-value expression selected by `from_node_with(...)`.
    pub from_node_with: Option<Expr>,
    /// Callable description expression selected by `describe_with(...)`.
    pub describe_with: Option<Expr>,
    /// Callable borrowed-value-to-Node expression selected by `to_node_with(...)`.
    pub to_node_with: Option<Expr>,
}

#[derive(Default)]
pub enum FieldFallback {
    #[default]
    Unspecified,
    Expression(Expr),
    FromDefaults,
}

impl FieldAttrs {
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
                    if let Some(first_span) = result.rename_span {
                        return Err(conflicting_declaration_error(
                            lit.span(),
                            first_span,
                            "duplicate `rename` field attribute",
                        ));
                    }
                    result.rename = Some(lit.value());
                    result.rename_span = Some(lit.span());
                    Ok(())
                } else if meta.path.is_ident("via") {
                    Err(meta.error("`via(...)` has been removed; use `with(module)` or individual function hooks"))
                } else if meta.path.is_ident("with") {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    if content.is_empty() {
                        return Err(content.error("`with(...)` requires one module path"));
                    }
                    let module: syn::Path = content.parse()?;
                    if !content.is_empty() {
                        return Err(
                            content.error("`with(...)` accepts exactly one module path")
                        );
                    }
                    if let Some(first_span) = result.with_span {
                        return Err(conflicting_declaration_error(
                            meta.path.span(),
                            first_span,
                            "duplicate `with` field attribute",
                        ));
                    }
                    result.with = Some(module);
                    result.with_span = Some(meta.path.span());
                    Ok(())
                } else if meta.path.is_ident("from_node_with") {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let expression: Expr = content.parse()?;
                    if !content.is_empty() {
                        return Err(content.error("`from_node_with(...)` accepts exactly one callable expression"));
                    }
                    set_field_hook(&mut result.from_node_with, expression, "from_node_with")
                } else if meta.path.is_ident("describe_with") {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let expression: Expr = content.parse()?;
                    if !content.is_empty() {
                        return Err(content.error("`describe_with(...)` accepts exactly one callable expression"));
                    }
                    set_field_hook(&mut result.describe_with, expression, "describe_with")
                } else if meta.path.is_ident("to_node_with") {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let expression: Expr = content.parse()?;
                    if !content.is_empty() {
                        return Err(content.error("`to_node_with(...)` accepts exactly one callable expression"));
                    }
                    set_field_hook(&mut result.to_node_with, expression, "to_node_with")
                } else {
                    Err(meta.error("unknown scry field attribute"))
                }
            })?;
        }

        result.validate_module_hooks()?;
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

    fn validate_for_positional(&self) -> Result<()> {
        if let Some(span) = self.rename_span {
            return Err(syn::Error::new(
                span,
                "`rename` is not supported on positional fields; positional fields use indices",
            ));
        }

        let message = match self.fallback {
            FieldFallback::Unspecified => return Ok(()),
            FieldFallback::Expression(_) => {
                "`default = EXPR` is not supported on positional fields; positional fields must \
                 be present"
            }
            FieldFallback::FromDefaults => {
                "`from_defaults` is not supported on positional fields; positional fields must \
                 be present"
            }
        };
        Err(syn::Error::new(self.fallback_span.unwrap_or_else(Span::call_site), message))
    }

    fn validate_module_hooks(&self) -> Result<()> {
        let Some(module_span) = self.with_span else {
            return Ok(());
        };

        for (name, hook) in [
            ("from_node_with", &self.from_node_with),
            ("to_node_with", &self.to_node_with),
            ("describe_with", &self.describe_with),
        ] {
            if let Some(hook) = hook {
                let mut error = syn::Error::new(
                    module_span,
                    format!("`with` cannot be combined with `{name}` on the same field"),
                );
                error.combine(syn::Error::new(hook.span(), "conflicting hook is here"));
                return Err(error);
            }
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

/// Sets a field hook once and identifies both declarations if it is repeated.
fn set_field_hook(hook: &mut Option<Expr>, expression: Expr, name: &str) -> Result<()> {
    if let Some(first) = hook {
        return Err(conflicting_declaration_error(
            expression.span(),
            first.span(),
            format!("duplicate `{name}` field attribute"),
        ));
    }

    *hook = Some(expression);
    Ok(())
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
    rename_span: Option<Span>,
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
                    if let Some(first_span) = result.rename_span {
                        return Err(conflicting_declaration_error(
                            lit.span(),
                            first_span,
                            "duplicate `rename` variant attribute",
                        ));
                    }
                    result.rename = Some(lit.value());
                    result.rename_span = Some(lit.span());
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
    rename_all_span: Option<Span>,
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
                    if let Some(first_span) = result.rename_all_span {
                        return Err(conflicting_declaration_error(
                            lit.span(),
                            first_span,
                            "duplicate `rename_all` enum attribute",
                        ));
                    }
                    result.rename_all = Some(parse_rename_all(&lit)?);
                    result.rename_all_span = Some(lit.span());
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
        Fields::Named(fields) => StructFields::Named(parse_named_fields(fields)?),
        Fields::Unnamed(fields) => StructFields::Tuple(parse_positional_fields(fields)?),
        Fields::Unit => {
            return Err(syn::Error::new_spanned(input, "Scry cannot be derived for unit structs"))
        }
    };

    Ok(StructInfo {
        ident: input.ident.clone(),
        generics: input.generics.clone(),
        fields,
        allow_unknown_keys: struct_attrs.allow_unknown_keys,
        doc: extract_doc(&input.attrs),
    })
}

fn parse_enum(input: &DeriveInput, data: &syn::DataEnum) -> Result<EnumInfo> {
    let enum_attrs = EnumAttrs::from_attrs(&input.attrs)?;
    let mut variants = Vec::new();
    let mut default_span = None;

    for variant in &data.variants {
        let data = match &variant.fields {
            Fields::Unit => VariantData::Unit,
            Fields::Unnamed(fields) => VariantData::Tuple(parse_positional_fields(fields)?),
            Fields::Named(fields) => VariantData::Struct(parse_named_fields(fields)?),
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

    validate_variant_names(&variants, enum_attrs.rename_all)?;

    Ok(EnumInfo {
        ident: input.ident.clone(),
        generics: input.generics.clone(),
        attrs: enum_attrs,
        variants,
        doc: extract_doc(&input.attrs),
    })
}

fn validate_variant_names(variants: &[VariantInfo], rename_all: Option<RenameAll>) -> Result<()> {
    let mut canonical_names = HashMap::new();
    let mut unit_spellings = HashMap::new();
    let mut payload_spellings = HashMap::new();

    for variant in variants {
        let name = variant
            .attrs
            .rename
            .clone()
            .unwrap_or_else(|| rename_all_variant(&variant.ident.to_string(), rename_all));
        let span = variant.attrs.rename_span.unwrap_or_else(|| variant.ident.span());

        if let Some(first_span) = canonical_names.insert(name.clone(), span) {
            return Err(conflicting_declaration_error(
                span,
                first_span,
                format!("duplicate Scry variant name {name:?}"),
            ));
        }

        let is_unit = matches!(variant.data, VariantData::Unit);
        let (spellings, kind) = if is_unit {
            (&mut unit_spellings, "unit")
        } else {
            (&mut payload_spellings, "payload")
        };

        for spelling in variant_spellings(&name) {
            let spelling = if is_unit {
                spelling.to_ascii_lowercase()
            } else {
                spelling
            };
            if let Some(first_span) = spellings.insert(spelling.clone(), span) {
                return Err(conflicting_declaration_error(
                    span,
                    first_span,
                    format!("conflicting Scry {kind} variant spelling {spelling:?}"),
                ));
            }
        }
    }

    Ok(())
}

fn parse_field(field: &syn::Field, member: Member) -> Result<FieldInfo> {
    let attrs = FieldAttrs::from_attrs(&field.attrs)?;
    if matches!(member, Member::Unnamed(_)) {
        attrs.validate_for_positional()?;
    }
    attrs.validate_for_type(&field.ty)?;

    Ok(FieldInfo {
        member,
        ty: field.ty.clone(),
        attrs,
        doc: extract_doc(&field.attrs),
    })
}

fn parse_named_fields(fields: &syn::FieldsNamed) -> Result<Vec<FieldInfo>> {
    let fields = fields
        .named
        .iter()
        .map(|field| {
            let member = Member::Named(field.ident.clone().expect("named fields have identifiers"));
            parse_field(field, member)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut keys = HashMap::new();

    for field in &fields {
        let key = field.config_key();
        let span = field.attrs.rename_span.unwrap_or_else(|| field.member.span());
        if let Some(first_span) = keys.insert(key.clone(), span) {
            return Err(conflicting_declaration_error(
                span,
                first_span,
                format!("duplicate Scry field key {key:?}"),
            ));
        }
    }

    Ok(fields)
}

fn parse_positional_fields(fields: &syn::FieldsUnnamed) -> Result<Vec<FieldInfo>> {
    fields
        .unnamed
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let member = Member::Unnamed(syn::Index {
                index: index as u32,
                span: field.span(),
            });
            parse_field(field, member)
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------- //
// Type Utilities

/// Checks whether a type is written as `Option<T>`.
pub fn is_option_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            return segment.ident == "Option"
                && matches!(
                    &segment.arguments,
                    syn::PathArguments::AngleBracketed(arguments)
                        if arguments.args.len() == 1
                            && matches!(arguments.args.first(), Some(syn::GenericArgument::Type(_)))
                );
        }
    }
    false
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

fn conflicting_declaration_error(
    span: Span,
    first_span: Span,
    message: impl Into<String>,
) -> syn::Error {
    let mut error = syn::Error::new(span, message.into());
    error.combine(syn::Error::new(first_span, "first declaration is here"));
    error
}

// ---------------------------------------------------------------------------------------------- //

#[cfg(test)]
mod tests;
