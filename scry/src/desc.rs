//! Description types for Scry configurations.
//!
//! Provides a single tree structure ([`Desc`]) for documenting config shapes,
//! with human-readable help rendering and path traversal.

use crate::key_path::{KeyPathError, Segment, TryIntoKeyPath};
use crate::{KeyPath, Node, NodeError};

// ---------------------------------------------------------------------------------------------- //
// Core Types

/// Description of a config value's structure.
#[derive(Debug, Clone, Default)]
pub struct Desc {
    /// Doc comment / description on the value itself.
    pub doc: String,
    /// The structural kind of this value.
    pub kind: DescKind,
    /// True if null is accepted in addition to the described shape.
    ///
    /// Retained as metadata rather than included in help output.
    pub nullable: bool,
    /// Advertises the type's Scry default independently of any enclosing field.
    pub type_default: Option<DefaultDesc>,
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

    /// String or single-key-map alternatives with actual configuration names.
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
    /// The policy applied when the field's key is missing.
    pub omission: Omission,
    /// Description of the field's value.
    pub value: Desc,
}

/// Describes the policy applied to a missing field key.
#[derive(Debug, Clone, Default)]
pub enum Omission {
    /// Requires the key to be present, independently of null acceptance.
    #[default]
    Required,
    /// Applies a declared fallback whose representation may be unavailable.
    Fallback(DefaultDesc),
}

/// Describes a default without evaluating it during help or description construction.
#[derive(Debug, Clone, Default)]
pub struct DefaultDesc {
    /// Static presentation information, independent of a concrete value.
    pub summary: Option<DefaultSummary>,
    /// Lazily supplies the default's configuration representation when known.
    pub value: Option<DefaultProvider>,
}

impl DefaultDesc {
    /// Describes a fallback whose configuration representation is unavailable.
    pub fn unknown() -> Self {
        Self::default()
    }

    /// Describes a default using static help text.
    pub fn text(display: impl Into<String>) -> Self {
        Self {
            summary: Some(DefaultSummary::Text(display.into())),
            value: None,
        }
    }

    /// Identifies an enum alternative selected by a default policy.
    pub fn variant(name: impl Into<String>) -> Self {
        Self {
            summary: Some(DefaultSummary::Variant(name.into())),
            value: None,
        }
    }

    /// Sets a lazy configuration-value provider.
    pub fn with_value(mut self, provider: DefaultProvider) -> Self {
        self.value = Some(provider);
        self
    }

    /// Retains static help information without advertising an executable provider.
    pub fn without_value(mut self) -> Self {
        self.value = None;
        self
    }

    /// Returns static help text without interpreting it as configuration data.
    pub fn display_text(&self) -> Option<&str> {
        match &self.summary {
            Some(DefaultSummary::Text(text)) => Some(text),
            _ => None,
        }
    }

    /// Returns the exact enum alternative selected by this default policy.
    pub fn variant_name(&self) -> Option<&str> {
        match &self.summary {
            Some(DefaultSummary::Variant(name)) => Some(name),
            _ => None,
        }
    }

    /// Evaluates the provider when a concrete configuration value is requested.
    pub fn get_value(&self) -> Result<Option<Node>, NodeError> {
        self.value.map_or(Ok(None), |provider| provider())
    }
}

/// Supplies static help information about a default policy.
#[derive(Debug, Clone)]
pub enum DefaultSummary {
    /// Shows literal help text without making it executable configuration.
    Text(String),
    /// Identifies an enum alternative by its exact configuration name.
    Variant(String),
}

/// Lazily obtains a default's known configuration value, preserving unavailable results.
pub type DefaultProvider = fn() -> Result<Option<Node>, NodeError>;

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
    Unit,

    /// Payload variant: written as a map with one key (e.g., `{ file: "path" }`).
    Payload {
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
            nullable: false,
            type_default: None,
        }
    }

    /// Creates a struct description.
    pub fn structure(fields: Vec<FieldDesc>) -> Self {
        Desc {
            doc: String::new(),
            kind: DescKind::Struct { fields },
            nullable: false,
            type_default: None,
        }
    }

    /// Creates an enum description.
    pub fn enumeration(variants: Vec<VariantDesc>) -> Self {
        Desc {
            doc: String::new(),
            kind: DescKind::Enum { variants },
            nullable: false,
            type_default: None,
        }
    }

    /// Creates a list description.
    pub fn list(item: Desc) -> Self {
        Desc {
            doc: String::new(),
            kind: DescKind::List {
                item: Box::new(item),
            },
            nullable: false,
            type_default: None,
        }
    }

    /// Creates a tuple description.
    pub fn tuple(items: Vec<Desc>) -> Self {
        Desc {
            doc: String::new(),
            kind: DescKind::Tuple { items },
            nullable: false,
            type_default: None,
        }
    }

    /// Sets the doc comment.
    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = doc.into();
        self
    }

    /// Marks the value as accepting null alongside its described shape.
    ///
    /// This does not change field omission, children, or enum choices. Applying it repeatedly
    /// has the same effect as applying it once.
    pub fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }

    /// Advertises the described type's default-construction metadata.
    pub fn with_type_default(mut self, default: DefaultDesc) -> Self {
        self.type_default = Some(default);
        self
    }

    /// Clears default-construction metadata while preserving the accepted shape.
    pub fn without_type_default(mut self) -> Self {
        self.type_default = None;
        self
    }

    /// Obtains the advertised type default's configuration value when available.
    pub fn template_default(&self) -> Result<Option<Node>, NodeError> {
        self.type_default.as_ref().map_or(Ok(None), DefaultDesc::get_value)
    }

    /// Resolves named-field fallbacks into a complete configuration map when possible.
    ///
    /// Aggregate providers opt into this operation explicitly. An immediate required field or
    /// unavailable provider stops preparation before any default expressions are evaluated.
    pub fn field_defaults(&self) -> Result<Option<Node>, NodeError> {
        let DescKind::Struct { fields } = &self.kind else {
            return Ok(None);
        };
        if fields.iter().any(|field| field.fallback().is_none_or(|default| default.value.is_none()))
        {
            return Ok(None);
        }
        let mut node = Node::empty_map();
        for field in fields {
            let key = KeyPath::from_keys([field.name.as_str()]);
            let Some(value) = field.template_default().map_err(|error| error.prepend_path(&key))?
            else {
                return Ok(None);
            };
            node.set_node(key, value)?;
        }
        Ok(Some(node))
    }

    /// Returns a concise value hint for help output.
    ///
    /// - Plain: returns the hint if present, else an empty string.
    /// - List: returns `list[hint]` if the item has a hint, else `list`.
    /// - Tuple: returns `tuple`.
    /// - Struct/Enum: returns an empty string. Their children describe the structure.
    ///
    /// Nullability is omitted at every level. This label is a reading aid, rather than a complete
    /// account of accepted values. Exact null acceptance remains available in [`Self::nullable`].
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
                if variants.iter().all(|variant| matches!(variant.repr, VariantRepr::Unit)) =>
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
            omission: Omission::Required,
            value,
        }
    }

    /// Sets the doc comment.
    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = doc.into();
        self
    }

    /// Marks the field as omittable without replacing an existing fallback.
    pub fn optional(mut self) -> Self {
        if matches!(self.omission, Omission::Required) {
            self.omission = Omission::Fallback(DefaultDesc::unknown());
        }
        self
    }

    /// Sets the field's missing-value fallback metadata.
    pub fn with_fallback(mut self, default: DefaultDesc) -> Self {
        self.omission = Omission::Fallback(default);
        self
    }

    /// Returns whether the key may be omitted.
    pub fn is_optional(&self) -> bool {
        matches!(self.omission, Omission::Fallback(_))
    }

    /// Returns the field's missing-value fallback metadata.
    pub fn fallback(&self) -> Option<&DefaultDesc> {
        match &self.omission {
            Omission::Required => None,
            Omission::Fallback(default) => Some(default),
        }
    }

    /// Returns static default text without evaluating a provider.
    pub fn default_display(&self) -> Option<&str> {
        self.fallback().and_then(DefaultDesc::display_text)
    }

    /// Obtains the fallback's concrete configuration value when available.
    pub fn template_default(&self) -> Result<Option<Node>, NodeError> {
        self.fallback().map_or(Ok(None), DefaultDesc::get_value)
    }
}

impl VariantDesc {
    /// Creates a unit variant description.
    pub fn unit(name: impl Into<String>) -> Self {
        VariantDesc {
            name: name.into(),
            doc: String::new(),
            repr: VariantRepr::Unit,
        }
    }

    /// Creates a payload variant description.
    pub fn payload(name: impl Into<String>, payload: Desc) -> Self {
        VariantDesc {
            name: name.into(),
            doc: String::new(),
            repr: VariantRepr::Payload { payload },
        }
    }

    /// Sets the doc comment.
    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = doc.into();
        self
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
    /// Maximum entry levels to render. None means unlimited.
    /// A limit of one shows immediate entries and marks omitted children with an ellipsis.
    pub max_depth: Option<usize>,
}

/// Symbols used in description tree rendering.
#[derive(Debug, Clone)]
pub struct DisplaySymbols {
    /// Symbol for required fields.
    pub required: &'static str,
    /// Symbol for optional fields.
    pub optional: &'static str,
    /// Symbol for the Scry default enum variant.
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
    /// Formats a human-readable help overview with default config.
    pub fn display(&self) -> String {
        self.display_with(&DisplayConfig::default())
    }

    /// Formats the description as a string with custom config.
    pub fn display_with(&self, config: &DisplayConfig) -> String {
        self.display_in_context(config, self.type_default.as_ref())
    }

    /// Formats a value overview using its enclosing default policy.
    fn display_in_context(&self, config: &DisplayConfig, default: Option<&DefaultDesc>) -> String {
        // First pass: collect all lines
        let mut lines = Vec::new();
        if matches!(self.kind, DescKind::Plain { .. }) {
            let label = self.type_label();
            if !label.is_empty() || !self.doc.is_empty() {
                lines.push(LineData {
                    left_side: label,
                    doc: self.doc.clone(),
                });
            }
        }
        self.collect_lines(&mut lines, &[], default, config);

        render_lines(lines, config)
    }

    /// Collects lines from the description tree.
    fn collect_lines(
        &self,
        lines: &mut Vec<LineData>,
        ancestors: &[bool],
        default: Option<&DefaultDesc>,
        config: &DisplayConfig,
    ) {
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
                    variant.collect_line(
                        lines,
                        ancestors,
                        is_last,
                        default.and_then(DefaultDesc::variant_name),
                        config,
                    );
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
                item.collect_lines(lines, ancestors, None, config);
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
    let within_depth = config.max_depth.is_none_or(|max| ancestors.len() + 1 < max);
    if !item.is_leaf() && !within_depth {
        label.push_str(" …");
    }

    // Build the full left side
    let left_side = format!("{}{} {}", prefix, branch, label);

    lines.push(LineData {
        left_side,
        doc: item.doc.clone(),
    });

    // Collect expandable children while another entry level fits the depth limit.
    if !item.is_leaf() && within_depth {
        let mut new_ancestors = ancestors.to_vec();
        new_ancestors.push(!is_last);
        item.collect_lines(lines, &new_ancestors, None, config);
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
        let branch = if self.is_optional() {
            config.symbols.optional
        } else {
            config.symbols.required
        };

        // Build the label: "name: type (default)"
        let mut label = self.build_label(config);
        let within_depth = config.max_depth.is_none_or(|max| ancestors.len() + 1 < max);
        if !self.value.is_leaf() && !within_depth {
            label.push_str(" …");
        }

        // Build the full left side
        let left_side = format!("{}{} {}", prefix, branch, label);

        // Collect this line
        lines.push(LineData {
            left_side,
            doc: self.doc.clone(),
        });

        // Collect expandable children while another entry level fits the depth limit.
        if !self.value.is_leaf() && within_depth {
            let mut new_ancestors = ancestors.to_vec();
            new_ancestors.push(!is_last);
            self.value.collect_lines(lines, &new_ancestors, self.fallback(), config);
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
        if let Some(default_val) = self.default_display() {
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
        default_variant: Option<&str>,
        config: &DisplayConfig,
    ) {
        // Build the tree prefix from ancestors
        let prefix = build_tree_prefix(ancestors, config);

        // Select the branch character based on default status
        let branch = if default_variant == Some(self.name.as_str()) {
            config.symbols.default_variant
        } else {
            config.symbols.variant
        };

        // Build the label
        let mut label = self.build_label();
        let within_depth = config.max_depth.is_none_or(|max| ancestors.len() + 1 < max);
        if let VariantRepr::Payload { payload, .. } = &self.repr {
            if !payload.is_leaf() && !within_depth {
                label.push_str(" …");
            }
        }

        // Build the full left side
        let left_side = format!("{}{} {}", prefix, branch, label);

        // Collect this line
        lines.push(LineData {
            left_side,
            doc: self.doc.clone(),
        });

        // Collect expandable payload children while another entry level fits the depth limit.
        if let VariantRepr::Payload { payload, .. } = &self.repr {
            if !payload.is_leaf() && within_depth {
                let mut new_ancestors = ancestors.to_vec();
                new_ancestors.push(!is_last);
                payload.collect_lines(lines, &new_ancestors, None, config);
            }
        }
    }

    fn build_label(&self) -> String {
        match &self.repr {
            VariantRepr::Unit => self.name.clone(),
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

/// Formats collected help lines using a shared comment column.
fn render_lines(lines: Vec<LineData>, config: &DisplayConfig) -> String {
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

// ---------------------------------------------------------------------------------------------- //
// Path Traversal

/// A reference to an entry (field or variant) in the description tree.
#[derive(Debug, Clone)]
pub enum EntryRef<'a> {
    /// A struct field.
    Field(&'a FieldDesc),
    /// An enum variant.
    Variant {
        /// The enum alternative.
        variant: &'a VariantDesc,
        /// The default policy applicable at this use site.
        default: Option<&'a DefaultDesc>,
    },
    /// A tuple element.
    TupleElem {
        /// The element index.
        index: usize,
        /// The element's description.
        value: &'a Desc,
    },
}

impl<'a> EntryRef<'a> {
    /// Returns the doc string for this entry.
    pub fn doc(&self) -> &str {
        match self {
            EntryRef::Field(field) => &field.doc,
            EntryRef::Variant { variant, .. } => &variant.doc,
            EntryRef::TupleElem { value, .. } => &value.doc,
        }
    }

    /// Displays this entry using default configuration.
    pub fn display(&self) -> String {
        self.display_with(&DisplayConfig::default())
    }

    /// Displays this entry using custom configuration.
    pub fn display_with(&self, config: &DisplayConfig) -> String {
        let mut lines = Vec::new();
        match self {
            EntryRef::Field(field) => field.collect_line(&mut lines, &[], true, config),
            EntryRef::Variant { variant, default } => variant.collect_line(
                &mut lines,
                &[],
                true,
                default.and_then(DefaultDesc::variant_name),
                config,
            ),
            EntryRef::TupleElem { index, value } => {
                collect_tuple_elem_line(*index, value, &mut lines, &[], true, config);
            }
        }
        render_lines(lines, config)
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
    pub desc_at_prefix: Box<Desc>,
    /// The default policy applicable at the valid prefix.
    pub default_at_prefix: Option<DefaultDesc>,
}

impl UnknownPathError {
    /// Creates a new unknown path error.
    fn new(
        prefix: &[&str],
        key: impl Into<String>,
        desc: Desc,
        default: Option<&DefaultDesc>,
    ) -> Self {
        Self {
            valid_prefix: prefix.join("."),
            invalid_key: key.into(),
            desc_at_prefix: Box::new(desc),
            default_at_prefix: default.cloned(),
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
            write!(
                f,
                "\n\nAvailable{}:\n{}",
                at_str,
                self.desc_at_prefix.display_in_context(&config, self.default_at_prefix.as_ref())
            )?;
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
        self.entry_at_segments(&segments, self.type_default.as_ref())
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
        self.validate_segments(&segments, &[], self.type_default.as_ref())
    }

    /// Finds an entry using path segments.
    fn entry_at_segments<'a>(
        &'a self,
        segments: &[&Segment],
        default: Option<&'a DefaultDesc>,
    ) -> Option<EntryRef<'a>> {
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
                        item.entry_at_segments(&segments[1..], None)
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
                        item.entry_at_segments(&segments[1..], None)
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
                    field.value.entry_at_segments(&segments[1..], field.fallback())
                }
            }
            DescKind::Enum { variants } => {
                let variant = variants.iter().find(|v| v.name == key)?;
                if segments.len() == 1 {
                    Some(EntryRef::Variant { variant, default })
                } else {
                    // Can only traverse into payload variants
                    match &variant.repr {
                        VariantRepr::Payload { payload, .. } => {
                            payload.entry_at_segments(&segments[1..], None)
                        }
                        VariantRepr::Unit => None,
                    }
                }
            }
            DescKind::Tuple { .. } => {
                // Tuples require index access, not key access
                None
            }
            DescKind::List { item } => {
                // For lists, apply the segment to the item description
                item.entry_at_segments(segments, None)
            }
            DescKind::Plain { .. } => None,
        }
    }

    /// Validates path segments, tracking the valid prefix.
    fn validate_segments(
        &self,
        segments: &[&Segment],
        prefix: &[&str],
        default: Option<&DefaultDesc>,
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
                            default,
                        )
                        .into());
                    }
                    if segments.len() > 1 {
                        // Continue validation into the tuple element
                        // (We don't add the index to prefix since it's already validated)
                        items[*i].validate_segments(&segments[1..], prefix, None)
                    } else {
                        Ok(())
                    }
                }
                DescKind::List { item } => {
                    // Lists accept any index (bounds checked at runtime on actual data).
                    if segments.len() > 1 {
                        item.validate_segments(&segments[1..], prefix, None)
                    } else {
                        Ok(())
                    }
                }
                // Struct, Enum, Plain do not support index access.
                _ => {
                    let msg = format!("[{}] (index access requires a list or tuple)", i);
                    Err(UnknownPathError::new(prefix, msg, self.clone(), default).into())
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
                            f.value.validate_segments(&segments[1..], &new_prefix, f.fallback())
                        } else {
                            Ok(())
                        }
                    }
                    None => Err(UnknownPathError::new(prefix, key, self.clone(), default).into()),
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
                                    payload.validate_segments(&segments[1..], &new_prefix, None)
                                }
                                VariantRepr::Unit => {
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
                                        None,
                                    )
                                    .into())
                                }
                            }
                        } else {
                            Ok(())
                        }
                    }
                    None => Err(UnknownPathError::new(prefix, key, self.clone(), default).into()),
                }
            }
            DescKind::Tuple { .. } => {
                // Tuples require index access, not key access
                let msg = format!("'{}' (tuple requires index access like [0])", key);
                Err(UnknownPathError::new(prefix, msg, self.clone(), default).into())
            }
            DescKind::List { item } => {
                // Apply segment to item
                item.validate_segments(segments, prefix, None)
            }
            DescKind::Plain { .. } => {
                // Plain types can't have children
                Err(UnknownPathError::new(prefix, key, self.clone(), default).into())
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Tests

#[cfg(test)]
mod tests;
