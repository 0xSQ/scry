//! Description types for Scry configurations.
//!
//! Provides a single tree structure ([`Desc`]) for documenting config shapes,
//! with integrated rendering and path traversal.

use crate::key_path::{KeyPathError, Segment, TryIntoKeyPath};
use std::marker::PhantomData;

// ---------------------------------------------------------------------------------------------- //
// Core Types

/// Description of a config value's structure.
#[derive(Debug, Clone, Default)]
pub struct Desc {
    /// Doc comment / description on the value itself.
    pub doc: String,
    /// The structural kind of this value.
    pub kind: DescKind,
}

/// The structural kind of a described value.
#[derive(Debug, Clone)]
pub enum DescKind {
    /// A plain type label with no expandable structure.
    ///
    /// Used for primitives or simple custom types.
    Plain {
        /// Type hint for display (e.g., "u32", "string", etc).
        hint: Option<String>,
    },

    /// Struct-like map with fixed keys.
    Struct {
        /// The struct's fields.
        fields: Vec<FieldDesc>,
    },

    /// Enum with named variants.
    Enum {
        /// The enum's variants.
        variants: Vec<VariantDesc>,
    },

    /// List/array/set of items.
    List {
        /// Description of list items.
        item: Box<Desc>,
    },

    /// Fixed-size tuple with heterogeneous elements.
    Tuple {
        /// Description of each tuple element, in order.
        items: Vec<Desc>,
    },
}

impl Default for DescKind {
    fn default() -> Self {
        DescKind::Plain { hint: None }
    }
}

/// Description of a struct field.
#[derive(Debug, Clone)]
pub struct FieldDesc {
    /// Field name as it appears in config.
    pub name: String,
    /// Doc comment for this field.
    pub doc: String,
    /// True if field is `Option<T>` or has a default.
    pub optional: bool,
    /// Display string for the default value, if any.
    pub default_expr: Option<String>,
    /// Description of the field's value.
    pub value: Desc,
}

/// Description of an enum variant.
#[derive(Debug, Clone)]
pub struct VariantDesc {
    /// Variant name as it appears in config.
    pub name: String,
    /// Doc comment for this variant.
    pub doc: String,
    /// How this variant is represented in config.
    pub repr: VariantRepr,
}

/// How an enum variant is represented in config.
#[derive(Debug, Clone)]
pub enum VariantRepr {
    /// Unit variant: written as a bare string in config (e.g., `"auto"`).
    Unit {
        /// True if this is the default variant.
        is_default: bool,
    },

    /// Payload variant: written as a map with one key (e.g., `{ file: "path" }`).
    Payload {
        /// True if this is the default variant.
        is_default: bool,
        /// Description of the payload.
        payload: Desc,
    },
}

// ---------------------------------------------------------------------------------------------- //
// Constructors

impl Desc {
    /// Creates a plain description with a type hint.
    pub fn plain(hint: impl Into<String>) -> Self {
        Desc {
            doc: String::new(),
            kind: DescKind::Plain {
                hint: Some(hint.into()),
            },
        }
    }

    /// Creates a struct description.
    pub fn structure(fields: Vec<FieldDesc>) -> Self {
        Desc {
            doc: String::new(),
            kind: DescKind::Struct { fields },
        }
    }

    /// Creates an enum description.
    pub fn enumeration(variants: Vec<VariantDesc>) -> Self {
        Desc {
            doc: String::new(),
            kind: DescKind::Enum { variants },
        }
    }

    /// Creates a list description.
    pub fn list(item: Desc) -> Self {
        Desc {
            doc: String::new(),
            kind: DescKind::List {
                item: Box::new(item),
            },
        }
    }

    /// Creates a tuple description.
    pub fn tuple(items: Vec<Desc>) -> Self {
        Desc {
            doc: String::new(),
            kind: DescKind::Tuple { items },
        }
    }

    /// Sets the doc comment.
    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = doc.into();
        self
    }

    /// Returns the type label for display purposes.
    ///
    /// - Plain: returns the hint if present, else empty string
    /// - List: returns "list\[hint\]" if item has a hint, else "list"
    /// - Tuple: returns "tuple"
    /// - Struct/Enum: returns empty string
    pub fn type_label(&self) -> String {
        match &self.kind {
            DescKind::Plain { hint } => hint.clone().unwrap_or_default(),
            DescKind::Struct { .. } => String::new(),
            DescKind::Enum { .. } => String::new(),
            DescKind::Tuple { .. } => "tuple".to_string(),
            DescKind::List { item } => {
                let inner = item.type_label();
                if inner.is_empty() {
                    "list".to_string()
                } else {
                    format!("list[{}]", inner)
                }
            }
        }
    }

    /// Returns true if this description has no expandable children.
    pub fn is_leaf(&self) -> bool {
        match &self.kind {
            DescKind::Plain { .. } => true,
            DescKind::Struct { fields, .. } => fields.is_empty(),
            DescKind::Enum { variants } => variants.is_empty(),
            DescKind::Tuple { items } => items.is_empty(),
            DescKind::List { item } => item.is_leaf(),
        }
    }

    /// Returns the variants if this is an enum with only unit variants.
    pub fn unit_enum_variants(&self) -> Option<&[VariantDesc]> {
        match &self.kind {
            DescKind::Enum { variants }
                if variants
                    .iter()
                    .all(|variant| matches!(variant.repr, VariantRepr::Unit { .. })) =>
            {
                Some(variants)
            }
            _ => None,
        }
    }
}

impl FieldDesc {
    /// Creates a new field description.
    pub fn new(name: impl Into<String>, value: Desc) -> Self {
        FieldDesc {
            name: name.into(),
            doc: String::new(),
            optional: false,
            default_expr: None,
            value,
        }
    }

    /// Sets the doc comment.
    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = doc.into();
        self
    }

    /// Marks the field as optional.
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Sets a default value expression.
    pub fn with_default(mut self, expr: impl Into<String>) -> Self {
        self.default_expr = Some(expr.into());
        self.optional = true; // Having a default implies optional
        self
    }
}

impl VariantDesc {
    /// Creates a unit variant description.
    pub fn unit(name: impl Into<String>, is_default: bool) -> Self {
        VariantDesc {
            name: name.into(),
            doc: String::new(),
            repr: VariantRepr::Unit { is_default },
        }
    }

    /// Creates a payload variant description.
    pub fn payload(name: impl Into<String>, is_default: bool, payload: Desc) -> Self {
        VariantDesc {
            name: name.into(),
            doc: String::new(),
            repr: VariantRepr::Payload {
                is_default,
                payload,
            },
        }
    }

    /// Sets the doc comment.
    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = doc.into();
        self
    }

    /// Returns true if this is the default variant.
    pub fn is_default(&self) -> bool {
        match &self.repr {
            VariantRepr::Unit { is_default } => *is_default,
            VariantRepr::Payload { is_default, .. } => *is_default,
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Display Configuration

/// Configuration for description display formatting.
#[derive(Debug, Clone)]
pub struct DisplayConfig {
    /// Spaces per indentation level after the tree character.
    pub indent_spaces: usize,
    /// Minimum spaces between content and comment.
    pub comment_buffer: usize,
    /// Maximum column for comments.
    pub max_comment_column: usize,
    /// Symbols used in tree rendering.
    pub symbols: DisplaySymbols,
    /// Maximum depth to render. None means unlimited.
    pub max_depth: Option<usize>,
}

/// Symbols used in description tree rendering.
#[derive(Debug, Clone)]
pub struct DisplaySymbols {
    /// Symbol for required fields.
    pub required: &'static str,
    /// Symbol for optional fields.
    pub optional: &'static str,
    /// Symbol for the default enum variant.
    pub default_variant: &'static str,
    /// Symbol for non-default enum variants.
    pub variant: &'static str,
    /// Symbol for default values.
    pub default_value: &'static str,
    /// Vertical line for tree indentation.
    pub tree_line: &'static str,
    /// Prefix for comments.
    pub comment: &'static str,
}

impl Default for DisplaySymbols {
    fn default() -> Self {
        Self {
            required: "◆",
            optional: "◇",
            default_variant: "»",
            variant: "›",
            default_value: "→",
            tree_line: "┊",
            comment: "‣ ",
        }
    }
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            indent_spaces: 2,
            comment_buffer: 3,
            max_comment_column: 50,
            symbols: DisplaySymbols::default(),
            max_depth: None,
        }
    }
}

impl DisplayConfig {
    /// Creates a config with the specified indentation.
    pub fn with_indent(indent_spaces: usize) -> Self {
        Self {
            indent_spaces,
            ..Default::default()
        }
    }

    /// Sets custom symbols for the display.
    pub fn with_symbols(mut self, symbols: DisplaySymbols) -> Self {
        self.symbols = symbols;
        self
    }

    /// Sets the maximum depth to render.
    pub fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = Some(max_depth);
        self
    }
}

// ---------------------------------------------------------------------------------------------- //
// Rendering

/// A single line to be rendered (collected in first pass).
struct LineData {
    left_side: String,
    doc: String,
}

impl Desc {
    /// Formats the description as a string with default config.
    pub fn display(&self) -> String {
        self.display_with(&DisplayConfig::default())
    }

    /// Formats the description as a string with custom config.
    pub fn display_with(&self, config: &DisplayConfig) -> String {
        // First pass: collect all lines
        let mut lines = Vec::new();
        self.collect_lines(&mut lines, &[], config);

        // Find max width of left sides (only for lines with descriptions)
        let max_width = lines
            .iter()
            .filter(|line| !line.doc.is_empty())
            .map(|line| line.left_side.chars().count())
            .max()
            .unwrap_or(0);

        // Calculate comment column: max width + buffer, capped at max_comment_column
        let comment_column = (max_width + config.comment_buffer).min(config.max_comment_column);

        // Second pass: format output
        let mut out = String::new();
        for line in lines {
            if line.doc.is_empty() {
                out.push_str(&line.left_side);
                out.push('\n');
            } else {
                let width = line.left_side.chars().count();
                let padding = if width < comment_column {
                    comment_column - width
                } else {
                    config.comment_buffer
                };
                out.push_str(&line.left_side);
                for _ in 0..padding {
                    out.push(' ');
                }
                out.push_str(config.symbols.comment);
                out.push_str(&line.doc);
                out.push('\n');
            }
        }
        out
    }

    /// Collects lines from the description tree.
    fn collect_lines(&self, lines: &mut Vec<LineData>, ancestors: &[bool], config: &DisplayConfig) {
        match &self.kind {
            DescKind::Struct { fields, .. } => {
                let count = fields.len();
                for (i, field) in fields.iter().enumerate() {
                    let is_last = i == count - 1;
                    field.collect_line(lines, ancestors, is_last, config);
                }
            }
            DescKind::Enum { variants } => {
                let count = variants.len();
                for (i, variant) in variants.iter().enumerate() {
                    let is_last = i == count - 1;
                    variant.collect_line(lines, ancestors, is_last, config);
                }
            }
            DescKind::Tuple { items } => {
                let count = items.len();
                for (i, item) in items.iter().enumerate() {
                    let is_last = i == count - 1;
                    collect_tuple_elem_line(i, item, lines, ancestors, is_last, config);
                }
            }
            DescKind::List { item } => {
                // For list, show the item's children if it has any
                item.collect_lines(lines, ancestors, config);
            }
            DescKind::Plain { .. } => {
                // Plain types have no children to render
            }
        }
    }
}

/// Collects a tuple element's line data for rendering.
fn collect_tuple_elem_line(
    index: usize,
    item: &Desc,
    lines: &mut Vec<LineData>,
    ancestors: &[bool],
    is_last: bool,
    config: &DisplayConfig,
) {
    // Build the tree prefix from ancestors
    let prefix = build_tree_prefix(ancestors, config);

    // Tuple elements are always required
    let branch = config.symbols.required;

    // Build the label: "0: type"
    let mut label = index.to_string();
    let hint = item.type_label();
    if !hint.is_empty() {
        label.push_str(": ");
        label.push_str(&hint);
    }

    // Build the full left side
    let left_side = format!("{}{} {}", prefix, branch, label);

    lines.push(LineData {
        left_side,
        doc: item.doc.clone(),
    });

    // Collect children recursively if the item has expandable children
    // and we haven't reached the depth limit
    // max_depth=1 means show 1 level (depth 0 only), max_depth=2 means show 2 levels, etc.
    let within_depth = config.max_depth.is_none_or(|max| ancestors.len() + 1 < max);
    if !item.is_leaf() && within_depth {
        let mut new_ancestors = ancestors.to_vec();
        new_ancestors.push(!is_last);
        item.collect_lines(lines, &new_ancestors, config);
    }
}

impl FieldDesc {
    /// Collects this field's line data for rendering.
    fn collect_line(
        &self,
        lines: &mut Vec<LineData>,
        ancestors: &[bool],
        is_last: bool,
        config: &DisplayConfig,
    ) {
        // Build the tree prefix from ancestors
        let prefix = build_tree_prefix(ancestors, config);

        // Select the branch character based on optionality
        let branch = if self.optional {
            config.symbols.optional
        } else {
            config.symbols.required
        };

        // Build the label: "name: type (default)"
        let label = self.build_label(config);

        // Build the full left side
        let left_side = format!("{}{} {}", prefix, branch, label);

        // Collect this line
        lines.push(LineData {
            left_side,
            doc: self.doc.clone(),
        });

        // Collect children recursively if the value has expandable children
        // and we haven't reached the depth limit
        // max_depth=1 means show 1 level (depth 0 only), max_depth=2 means show 2 levels, etc.
        let within_depth = config.max_depth.is_none_or(|max| ancestors.len() + 1 < max);
        if !self.value.is_leaf() && within_depth {
            let mut new_ancestors = ancestors.to_vec();
            new_ancestors.push(!is_last);
            self.value.collect_lines(lines, &new_ancestors, config);
        }
    }

    fn build_label(&self, config: &DisplayConfig) -> String {
        let mut label = self.name.clone();

        // Add type hint if present
        let hint = self.value.type_label();
        if !hint.is_empty() {
            label.push_str(": ");
            label.push_str(&hint);
        }

        // Add default value if present
        if let Some(default_val) = &self.default_expr {
            label.push(' ');
            label.push_str(config.symbols.default_value);
            label.push(' ');
            label.push_str(default_val);
        }

        label
    }
}

impl VariantDesc {
    /// Collects this variant's line data for rendering.
    fn collect_line(
        &self,
        lines: &mut Vec<LineData>,
        ancestors: &[bool],
        is_last: bool,
        config: &DisplayConfig,
    ) {
        // Build the tree prefix from ancestors
        let prefix = build_tree_prefix(ancestors, config);

        // Select the branch character based on default status
        let branch = if self.is_default() {
            config.symbols.default_variant
        } else {
            config.symbols.variant
        };

        // Build the label
        let label = self.build_label();

        // Build the full left side
        let left_side = format!("{}{} {}", prefix, branch, label);

        // Collect this line
        lines.push(LineData {
            left_side,
            doc: self.doc.clone(),
        });

        // Collect payload children if this is a payload variant
        // and we haven't reached the depth limit
        // max_depth=1 means show 1 level (depth 0 only), max_depth=2 means show 2 levels, etc.
        let within_depth = config.max_depth.is_none_or(|max| ancestors.len() + 1 < max);
        if let VariantRepr::Payload { payload, .. } = &self.repr {
            if !payload.is_leaf() && within_depth {
                let mut new_ancestors = ancestors.to_vec();
                new_ancestors.push(!is_last);
                payload.collect_lines(lines, &new_ancestors, config);
            }
        }
    }

    fn build_label(&self) -> String {
        match &self.repr {
            VariantRepr::Unit { .. } => self.name.clone(),
            VariantRepr::Payload { payload, .. } => {
                let hint = payload.type_label();
                if hint.is_empty() {
                    self.name.clone()
                } else {
                    format!("{}: {}", self.name, hint)
                }
            }
        }
    }
}

/// Builds the tree prefix string from ancestor information.
fn build_tree_prefix(ancestors: &[bool], config: &DisplayConfig) -> String {
    let mut prefix = String::new();
    let spaces = " ".repeat(config.indent_spaces);
    let spine_with_line = format!("{}{}", config.symbols.tree_line, spaces);
    let spine_blank = format!(" {}", spaces);
    for &has_more in ancestors {
        prefix.push_str(if has_more {
            &spine_with_line
        } else {
            &spine_blank
        });
    }
    prefix
}

// ---------------------------------------------------------------------------------------------- //
// Path Traversal

/// A reference to an entry (field or variant) in the description tree.
#[derive(Debug, Clone)]
pub enum EntryRef<'a> {
    /// A struct field.
    Field(&'a FieldDesc),
    /// An enum variant.
    Variant(&'a VariantDesc),
    /// A tuple element.
    TupleElem {
        /// The element index.
        index: usize,
        /// The element's description.
        value: &'a Desc,
    },
}

impl<'a> EntryRef<'a> {
    /// Converts this entry reference to an owned Desc for display.
    ///
    /// For fields, creates a struct desc with that single field.
    /// For variants, creates an enum desc with that single variant.
    /// For tuple elements, creates a struct desc with a numeric field name.
    pub fn to_desc(&self) -> Desc {
        match self {
            EntryRef::Field(field) => Desc::structure(vec![(*field).clone()]),
            EntryRef::Variant(variant) => Desc::enumeration(vec![(*variant).clone()]),
            EntryRef::TupleElem { index, value } => {
                Desc::structure(vec![FieldDesc::new(index.to_string(), (*value).clone())])
            }
        }
    }

    /// Returns the doc string for this entry.
    pub fn doc(&self) -> &str {
        match self {
            EntryRef::Field(field) => &field.doc,
            EntryRef::Variant(variant) => &variant.doc,
            EntryRef::TupleElem { value, .. } => &value.doc,
        }
    }

    /// Displays this entry using default configuration.
    pub fn display(&self) -> String {
        self.to_desc().display()
    }

    /// Displays this entry using custom configuration.
    pub fn display_with(&self, config: &DisplayConfig) -> String {
        self.to_desc().display_with(config)
    }
}

/// Error returned when a path doesn't match the description.
#[derive(Debug, thiserror::Error)]
pub enum DescPathError {
    /// The path string couldn't be parsed.
    #[error(transparent)]
    InvalidSyntax(#[from] KeyPathError),

    /// The path was valid syntax but doesn't exist in the description.
    #[error(transparent)]
    UnknownPath(#[from] UnknownPathError),
}

/// Error indicating a path was valid syntax but doesn't exist in the description.
#[derive(Debug)]
pub struct UnknownPathError {
    /// The valid portion of the path (empty if root-level error).
    pub valid_prefix: String,
    /// The key that wasn't found.
    pub invalid_key: String,
    /// The description at the valid prefix for displaying available entries.
    pub desc_at_prefix: Desc,
}

impl UnknownPathError {
    /// Creates a new unknown path error.
    fn new(prefix: &[&str], key: impl Into<String>, desc: Desc) -> Self {
        Self {
            valid_prefix: prefix.join("."),
            invalid_key: key.into(),
            desc_at_prefix: desc,
        }
    }
}

impl std::fmt::Display for UnknownPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.valid_prefix.is_empty() {
            write!(f, "Unknown key '{}'", self.invalid_key)?;
        } else {
            write!(f, "Unknown key '{}' in '{}'", self.invalid_key, self.valid_prefix)?;
        }
        if !self.desc_at_prefix.is_leaf() {
            let config = DisplayConfig::default().with_max_depth(1);
            let at_str = if self.valid_prefix.is_empty() {
                " in config".to_string()
            } else {
                format!(" at '{}'", self.valid_prefix)
            };
            write!(f, "\n\nAvailable{}:\n{}", at_str, self.desc_at_prefix.display_with(&config))?;
        }
        Ok(())
    }
}

impl std::error::Error for UnknownPathError {}

impl Desc {
    /// Finds the entry at the given path.
    ///
    /// Accepts any type that implements [`TryIntoKeyPath`], including strings
    /// (parsed using dotted syntax like "server.host"), `KeyPath` values, and arrays of keys.
    ///
    /// Returns `None` if the path doesn't match or leads to a leaf.
    pub fn entry_at_path(&self, path: impl TryIntoKeyPath) -> Option<EntryRef<'_>> {
        let key_path = path.try_into_key_path().ok()?;
        let segments: Vec<_> = key_path.iter().collect();
        self.entry_at_segments(&segments)
    }

    /// Validates a path against the description.
    ///
    /// Accepts any type that implements [`TryIntoKeyPath`], including strings
    /// (parsed using dotted syntax like "server.host"), `KeyPath` values, and arrays of keys.
    ///
    /// Returns `Ok(())` if valid, or `Err(DescPathError)` with details about the failure.
    pub fn validate_path(&self, path: impl TryIntoKeyPath) -> Result<(), DescPathError> {
        let key_path = path.try_into_key_path()?;
        let segments: Vec<_> = key_path.iter().collect();
        self.validate_segments(&segments, &[])
    }

    /// Finds an entry using path segments.
    fn entry_at_segments(&self, segments: &[&Segment]) -> Option<EntryRef<'_>> {
        let first = segments.first()?;

        // Handle index segments specially for tuples
        if let Segment::Index(i) = first {
            return match &self.kind {
                DescKind::Tuple { items } => {
                    let item = items.get(*i)?;
                    if segments.len() == 1 {
                        Some(EntryRef::TupleElem {
                            index: *i,
                            value: item,
                        })
                    } else {
                        item.entry_at_segments(&segments[1..])
                    }
                }
                DescKind::List { item } => {
                    // All list elements share the same type. Treat like a tuple element for display.
                    if segments.len() == 1 {
                        Some(EntryRef::TupleElem {
                            index: *i,
                            value: item,
                        })
                    } else {
                        item.entry_at_segments(&segments[1..])
                    }
                }
                // Struct, Enum, Plain do not support index access.
                _ => None,
            };
        }

        let key = match first {
            Segment::Key(k) => k.as_str(),
            Segment::Index(_) => unreachable!(), // Handled above
        };

        match &self.kind {
            DescKind::Struct { fields, .. } => {
                let field = fields.iter().find(|f| f.name == key)?;
                if segments.len() == 1 {
                    Some(EntryRef::Field(field))
                } else {
                    field.value.entry_at_segments(&segments[1..])
                }
            }
            DescKind::Enum { variants } => {
                let variant = variants.iter().find(|v| v.name == key)?;
                if segments.len() == 1 {
                    Some(EntryRef::Variant(variant))
                } else {
                    // Can only traverse into payload variants
                    match &variant.repr {
                        VariantRepr::Payload { payload, .. } => {
                            payload.entry_at_segments(&segments[1..])
                        }
                        VariantRepr::Unit { .. } => None,
                    }
                }
            }
            DescKind::Tuple { .. } => {
                // Tuples require index access, not key access
                None
            }
            DescKind::List { item } => {
                // For lists, apply the segment to the item description
                item.entry_at_segments(segments)
            }
            DescKind::Plain { .. } => None,
        }
    }

    /// Validates path segments, tracking the valid prefix.
    fn validate_segments(
        &self,
        segments: &[&Segment],
        prefix: &[&str],
    ) -> Result<(), DescPathError> {
        let Some(first) = segments.first() else {
            return Ok(());
        };

        // Handle index segments specially
        if let Segment::Index(i) = first {
            return match &self.kind {
                DescKind::Tuple { items } => {
                    // Tuple indices are semantic - bounds check required
                    if *i >= items.len() {
                        let valid_range = if items.is_empty() {
                            "empty tuple".to_string()
                        } else {
                            format!("valid: 0..{}", items.len() - 1)
                        };
                        return Err(UnknownPathError::new(
                            prefix,
                            format!("[{}] ({})", i, valid_range),
                            self.clone(),
                        )
                        .into());
                    }
                    if segments.len() > 1 {
                        // Continue validation into the tuple element
                        // (We don't add the index to prefix since it's already validated)
                        items[*i].validate_segments(&segments[1..], prefix)
                    } else {
                        Ok(())
                    }
                }
                DescKind::List { item } => {
                    // Lists accept any index (bounds checked at runtime on actual data).
                    if segments.len() > 1 {
                        item.validate_segments(&segments[1..], prefix)
                    } else {
                        Ok(())
                    }
                }
                // Struct, Enum, Plain do not support index access.
                _ => {
                    let msg = format!("[{}] (index access requires a list or tuple)", i);
                    Err(UnknownPathError::new(prefix, msg, self.clone()).into())
                }
            };
        }

        let key = match first {
            Segment::Key(k) => k.as_str(),
            Segment::Index(_) => unreachable!(), // Handled above
        };

        match &self.kind {
            DescKind::Struct { fields, .. } => {
                let field = fields.iter().find(|f| f.name == key);
                match field {
                    Some(f) => {
                        if segments.len() > 1 {
                            let mut new_prefix = prefix.to_vec();
                            new_prefix.push(key);
                            f.value.validate_segments(&segments[1..], &new_prefix)
                        } else {
                            Ok(())
                        }
                    }
                    None => Err(UnknownPathError::new(prefix, key, self.clone()).into()),
                }
            }
            DescKind::Enum { variants } => {
                let variant = variants.iter().find(|v| v.name == key);
                match variant {
                    Some(v) => {
                        if segments.len() > 1 {
                            match &v.repr {
                                VariantRepr::Payload { payload, .. } => {
                                    let mut new_prefix = prefix.to_vec();
                                    new_prefix.push(key);
                                    payload.validate_segments(&segments[1..], &new_prefix)
                                }
                                VariantRepr::Unit { .. } => {
                                    // Unit variants cannot have deeper paths
                                    let mut new_prefix = prefix.to_vec();
                                    new_prefix.push(key);
                                    let invalid_key = segments
                                        .get(1)
                                        .map(|s| match s {
                                            Segment::Key(k) => k.clone(),
                                            Segment::Index(i) => i.to_string(),
                                        })
                                        .unwrap_or_default();
                                    Err(UnknownPathError::new(
                                        &new_prefix,
                                        invalid_key,
                                        Desc::default(),
                                    )
                                    .into())
                                }
                            }
                        } else {
                            Ok(())
                        }
                    }
                    None => Err(UnknownPathError::new(prefix, key, self.clone()).into()),
                }
            }
            DescKind::Tuple { .. } => {
                // Tuples require index access, not key access
                let msg = format!("'{}' (tuple requires index access like [0])", key);
                Err(UnknownPathError::new(prefix, msg, self.clone()).into())
            }
            DescKind::List { item } => {
                // Apply segment to item
                item.validate_segments(segments, prefix)
            }
            DescKind::Plain { .. } => {
                // Plain types can't have children
                Err(UnknownPathError::new(prefix, key, self.clone()).into())
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Specialization Machinery

/// Probe for detecting Describe implementation via specialization.
pub struct DescProbe<T: ?Sized>(PhantomData<T>);

impl<T: ?Sized> DescProbe<T> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T: ?Sized> Default for DescProbe<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Fallback trait that returns None for types without Describe.
pub trait DescFallback {
    fn describe(&self) -> Option<Desc> {
        None
    }

    fn hint(&self) -> Option<String> {
        None
    }
}

impl<T: ?Sized> DescFallback for DescProbe<T> {}

/// Inherent impl for types that implement Describe.
impl<T: crate::Describe> DescProbe<T> {
    pub fn describe(&self) -> Option<Desc> {
        Some(T::describe())
    }

    pub fn hint(&self) -> Option<String> {
        let desc = T::describe();
        let label = desc.type_label();
        if label.is_empty() {
            None
        } else {
            Some(label)
        }
    }
}

/// Creates a probe for detecting Describe implementation.
pub fn make_desc_probe<T: ?Sized>() -> DescProbe<T> {
    DescProbe::new()
}

// ---------------------------------------------------------------------------------------------- //
// Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_struct_rendering() {
        let desc = Desc::structure(vec![
            FieldDesc::new("name", Desc::plain("string")).with_doc("The name"),
            FieldDesc::new("count", Desc::plain("u32")),
            FieldDesc::new("enabled", Desc::plain("bool")).optional().with_doc("Whether enabled"),
        ]);

        let output = desc.display();
        assert!(output.contains("◆ name: string"));
        assert!(output.contains("◆ count: u32"));
        assert!(output.contains("◇ enabled: bool"));
        assert!(output.contains("‣ The name"));
        assert!(output.contains("‣ Whether enabled"));
    }

    #[test]
    fn test_nested_struct_rendering() {
        let inner = Desc::structure(vec![
            FieldDesc::new("host", Desc::plain("string")),
            FieldDesc::new("port", Desc::plain("u16")).with_default("8080"),
        ]);

        let desc = Desc::structure(vec![
            FieldDesc::new("server", inner),
            FieldDesc::new("debug", Desc::plain("bool")),
        ]);

        let output = desc.display();
        assert!(output.contains("◆ server"));
        assert!(output.contains("◆ host: string"));
        assert!(output.contains("◇ port: u16 → 8080"));
        assert!(output.contains("◆ debug: bool"));
    }

    #[test]
    fn test_enum_rendering() {
        let desc = Desc::enumeration(vec![
            VariantDesc::unit("auto", true).with_doc("Automatic mode"),
            VariantDesc::unit("manual", false),
            VariantDesc::payload(
                "custom",
                false,
                Desc::structure(vec![FieldDesc::new("value", Desc::plain("u32"))]),
            ),
        ]);

        let output = desc.display();
        assert!(output.contains("» auto"));
        assert!(output.contains("› manual"));
        assert!(output.contains("› custom"));
        assert!(output.contains("‣ Automatic mode"));
    }

    #[test]
    fn test_list_rendering() {
        let desc = Desc::structure(vec![
            FieldDesc::new("tags", Desc::list(Desc::plain("string"))),
            FieldDesc::new(
                "items",
                Desc::list(Desc::structure(vec![FieldDesc::new("id", Desc::plain("u32"))])),
            ),
        ]);

        let output = desc.display();
        assert!(output.contains("tags: list[string]"));
        assert!(output.contains("items: list"));
        assert!(output.contains("◆ id: u32"));
    }

    #[test]
    fn test_type_label() {
        assert_eq!(Desc::plain("u32").type_label(), "u32");
        assert_eq!(Desc::default().type_label(), "");
        assert_eq!(Desc::list(Desc::plain("string")).type_label(), "list[string]");
        assert_eq!(Desc::list(Desc::default()).type_label(), "list");
    }

    // Path traversal tests

    #[test]
    fn test_entry_at_path_struct() {
        let desc = Desc::structure(vec![
            FieldDesc::new("name", Desc::plain("string")),
            FieldDesc::new(
                "server",
                Desc::structure(vec![
                    FieldDesc::new("host", Desc::plain("string")),
                    FieldDesc::new("port", Desc::plain("u16")),
                ]),
            ),
        ]);

        // Direct field
        let entry = desc.entry_at_path("name").unwrap();
        assert!(matches!(entry, EntryRef::Field(f) if f.name == "name"));

        // Nested field
        let entry = desc.entry_at_path("server.host").unwrap();
        assert!(matches!(entry, EntryRef::Field(f) if f.name == "host"));

        // Missing field
        assert!(desc.entry_at_path("missing").is_none());
        assert!(desc.entry_at_path("server.missing").is_none());
    }

    #[test]
    fn test_entry_at_path_enum() {
        let desc = Desc::enumeration(vec![
            VariantDesc::unit("auto", true),
            VariantDesc::payload(
                "custom",
                false,
                Desc::structure(vec![FieldDesc::new("value", Desc::plain("u32"))]),
            ),
        ]);

        // Unit variant
        let entry = desc.entry_at_path("auto").unwrap();
        assert!(matches!(entry, EntryRef::Variant(v) if v.name == "auto"));

        // Payload variant's field
        let entry = desc.entry_at_path("custom.value").unwrap();
        assert!(matches!(entry, EntryRef::Field(f) if f.name == "value"));

        // Can't traverse into unit variant
        assert!(desc.entry_at_path("auto.something").is_none());
    }

    #[test]
    fn test_validate_path_success() {
        let desc = Desc::structure(vec![
            FieldDesc::new("name", Desc::plain("string")),
            FieldDesc::new(
                "server",
                Desc::structure(vec![FieldDesc::new("host", Desc::plain("string"))]),
            ),
        ]);

        assert!(desc.validate_path("name").is_ok());
        assert!(desc.validate_path("server").is_ok());
        assert!(desc.validate_path("server.host").is_ok());
    }

    #[test]
    fn test_validate_path_failure() {
        let desc = Desc::structure(vec![FieldDesc::new("name", Desc::plain("string"))]);

        let err = desc.validate_path("missing").unwrap_err();
        match err {
            DescPathError::UnknownPath(UnknownPathError {
                invalid_key,
                valid_prefix,
                ..
            }) => {
                assert_eq!(invalid_key, "missing");
                assert!(valid_prefix.is_empty());
            }
            DescPathError::InvalidSyntax(_) => panic!("expected UnknownPath"),
        }
    }

    #[test]
    fn test_validate_path_nested_failure() {
        let desc = Desc::structure(vec![FieldDesc::new(
            "server",
            Desc::structure(vec![FieldDesc::new("host", Desc::plain("string"))]),
        )]);

        let err = desc.validate_path("server.missing").unwrap_err();
        match err {
            DescPathError::UnknownPath(UnknownPathError {
                invalid_key,
                valid_prefix,
                ..
            }) => {
                assert_eq!(invalid_key, "missing");
                assert_eq!(valid_prefix, "server");
            }
            DescPathError::InvalidSyntax(_) => panic!("expected UnknownPath"),
        }
    }

    #[test]
    fn test_validate_path_unit_variant_blocks_deeper() {
        let desc = Desc::enumeration(vec![VariantDesc::unit("auto", true)]);

        // Selecting unit variant is fine
        assert!(desc.validate_path("auto").is_ok());

        // But can't go deeper
        let err = desc.validate_path("auto.something").unwrap_err();
        match err {
            DescPathError::UnknownPath(UnknownPathError { valid_prefix, .. }) => {
                assert_eq!(valid_prefix, "auto");
            }
            DescPathError::InvalidSyntax(_) => panic!("expected UnknownPath"),
        }
    }

    #[test]
    fn test_display_with_max_depth() {
        // Create a nested structure: root { level1 { level2 { level3: scalar } } }
        let desc = Desc::structure(vec![FieldDesc::new(
            "level1",
            Desc::structure(vec![FieldDesc::new(
                "level2",
                Desc::structure(vec![FieldDesc::new("level3", Desc::plain("string"))]),
            )]),
        )]);

        // With max_depth=1, should only show level1, not level2 or level3
        let config = DisplayConfig::default().with_max_depth(1);
        let output = desc.display_with(&config);

        assert!(output.contains("level1"), "Should contain level1");
        assert!(!output.contains("level2"), "Should not contain level2 (beyond depth 1)");
        assert!(!output.contains("level3"), "Should not contain level3 (beyond depth 1)");
    }

    #[test]
    fn test_display_with_max_depth_2() {
        // Create a nested structure: root { level1 { level2 { level3: scalar } } }
        let desc = Desc::structure(vec![FieldDesc::new(
            "level1",
            Desc::structure(vec![FieldDesc::new(
                "level2",
                Desc::structure(vec![FieldDesc::new("level3", Desc::plain("string"))]),
            )]),
        )]);

        // With max_depth=2, should show level1 and level2, but not level3
        let config = DisplayConfig::default().with_max_depth(2);
        let output = desc.display_with(&config);

        assert!(output.contains("level1"), "Should contain level1");
        assert!(output.contains("level2"), "Should contain level2");
        assert!(!output.contains("level3"), "Should not contain level3 (beyond depth 2)");
    }

    #[test]
    fn test_display_unlimited_depth() {
        // Create a nested structure: root { level1 { level2 { level3: scalar } } }
        let desc = Desc::structure(vec![FieldDesc::new(
            "level1",
            Desc::structure(vec![FieldDesc::new(
                "level2",
                Desc::structure(vec![FieldDesc::new("level3", Desc::plain("string"))]),
            )]),
        )]);

        // With no max_depth, should show all levels
        let output = desc.display();

        assert!(output.contains("level1"), "Should contain level1");
        assert!(output.contains("level2"), "Should contain level2");
        assert!(output.contains("level3"), "Should contain level3");
    }

    #[test]
    fn test_path_error_shows_only_immediate_children() {
        // Create a nested structure where the error will occur at root level
        let desc = Desc::structure(vec![
            FieldDesc::new(
                "alpha",
                Desc::structure(vec![FieldDesc::new("nested", Desc::plain("string"))]),
            ),
            FieldDesc::new("beta", Desc::plain("string")),
        ]);

        let err = desc.validate_path("invalid").unwrap_err();
        let error_display = err.to_string();

        // Should show the immediate children (alpha, beta) but not nested children
        assert!(error_display.contains("alpha"), "Should show immediate child 'alpha'");
        assert!(error_display.contains("beta"), "Should show immediate child 'beta'");
        assert!(
            !error_display.contains("nested"),
            "Should not show nested children (depth limited)"
        );
    }
}
