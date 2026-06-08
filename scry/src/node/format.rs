//! Configuration file format handling.
//!
//! This module defines format identifiers, parser/writer registries, and conversion
//! logic between external config representations and [`Node`].
//!
//! # Overview
//!
//! - [`Format`] is the canonical format identifier type used by parser/writer registries.
//! - Built-in format ids include `rhai`.
//! - `json` is available when the `format-json` feature is enabled (enabled by default).
//! - `json5` is available when the `format-json5` feature is enabled (enabled by default).
//! - `toml` is available when the `format-toml` feature is enabled.
//! - `yaml` is available when the `format-yaml` feature is enabled.
//! - Custom format ids use [`Format::Custom`] via validated names.
//! - Parser dispatch is handled by [`FormatParserRegistry`].
//! - Writer dispatch is handled by [`FormatWriterRegistry`].
//!
//! Common entry points:
//! - [`Node::parse_file`](crate::node::Node::parse_file) for extension-based parsing.
//! - [`Node::parse_str`](crate::node::Node::parse_str) for explicit format parsing.
//! - [`Node::to_string_as`](crate::node::Node::to_string_as) for default-registry output.
//! - [`Node::to_format_string`](crate::node::Node::to_format_string) for writer-based output.
//!
//! # Basic Usage
//!
//! ```rust
//! use scry::node::Format;
//! use scry::Node;
//!
//! # #[cfg(feature = "format-json")]
//! # {
//! let node = Node::parse_str(r#"{ "name": "demo" }"#, Format::Json)?;
//! assert_eq!(node.req::<String>("name")?, "demo");
//! # }
//! # Ok::<(), scry::NodeError>(())
//! ```
//!
//! # Custom Parser Registration
//!
//! ```rust,ignore
//! use scry::node::{ConfigFormatParser, Format, FormatParserRegistryBuilder, Node, NodeError};
//!
//! #[derive(Debug, Clone, Copy)]
//! struct JsonAliasParser;
//!
//! impl ConfigFormatParser for JsonAliasParser {
//!     fn id(&self) -> Format {
//!         "json-alias".parse().unwrap()
//!     }
//!
//!     fn extensions(&self) -> &'static [&'static str] {
//!         &["jalias"]
//!     }
//!
//!     fn parse_str(&self, source: &str) -> Result<Node, NodeError> {
//!         Node::parse_str(source, Format::Json)
//!     }
//! }
//!
//! let registry = FormatParserRegistryBuilder::new().add(JsonAliasParser)?.build();
//! let node = Node::parse_file_with_registry("config.jalias", &registry)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Custom Writer Registration
//!
//! ```rust,ignore
//! use scry::node::{ConfigFormatWriter, Format, FormatWriterRegistryBuilder, Node, NodeError};
//!
//! #[derive(Debug, Clone, Copy)]
//! struct CompactJsonWriter;
//!
//! impl ConfigFormatWriter for CompactJsonWriter {
//!     fn id(&self) -> Format {
//!         "json-compact".parse().unwrap()
//!     }
//!
//!     fn write_node(&self, node: &Node) -> Result<String, NodeError> {
//!         let pretty_json = node.to_string_as(Format::Json)?;
//!         let value: serde_json::Value = serde_json::from_str(&pretty_json)
//!             .map_err(|e| NodeError::parse_format("JSON", e))?;
//!         serde_json::to_string(&value).map_err(|e| NodeError::serialize_format("JSON", e))
//!     }
//! }
//!
//! let writers = FormatWriterRegistryBuilder::new().add(CompactJsonWriter)?.build();
//! let out = Node::parse_str(r#"{ "a": 1 }"#, Format::Json)?
//!     .to_format_string("json-compact".parse()?, &writers)?;
//! assert_eq!(out, r#"{"a":1}"#);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::key_path::KeyPath;
use crate::node::{Kind, Node, NodeError, Value};
use crate::rhai::display_to_boxed;
use indexmap::IndexMap;
use rhai::Dynamic;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

// ---------------------------------------------------------------------------------------------- //
// Format

/// Identifier for a config format.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Format {
    #[cfg(feature = "format-json")]
    Json,
    #[cfg(feature = "format-json5")]
    Json5,
    Rhai,
    #[cfg(feature = "format-toml")]
    Toml,
    #[cfg(feature = "format-yaml")]
    Yaml,
    Custom(CustomFormatName),
}

/// Validated name for a user-defined custom format.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CustomFormatName(Box<str>);

impl Format {
    /// Creates a format from user input.
    ///
    /// Built-ins map to explicit variants; other ids become validated `Custom` formats.
    pub fn new(id: impl AsRef<str>) -> Result<Self, FormatError> {
        id.as_ref().parse()
    }

    /// Creates a custom format from user input.
    ///
    /// This rejects built-in format names.
    pub fn custom(id: impl AsRef<str>) -> Result<Self, FormatError> {
        let name = CustomFormatName::new(id)?;
        if builtin_format_from_str(name.as_str()).is_some() {
            return Err(FormatError::ReservedBuiltinFormatName {
                id: name.as_str().to_string(),
            });
        }
        Ok(Self::Custom(name))
    }

    /// Returns this format as its canonical lowercase id string.
    pub fn as_str(&self) -> &str {
        match self {
            #[cfg(feature = "format-json")]
            Format::Json => "json",
            #[cfg(feature = "format-json5")]
            Format::Json5 => "json5",
            Format::Rhai => "rhai",
            #[cfg(feature = "format-toml")]
            Format::Toml => "toml",
            #[cfg(feature = "format-yaml")]
            Format::Yaml => "yaml",
            Format::Custom(name) => name.as_str(),
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl AsRef<str> for Format {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<&Format> for Format {
    fn from(value: &Format) -> Self {
        value.clone()
    }
}

impl FromStr for Format {
    type Err = FormatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.to_ascii_lowercase();
        if let Some(format) = builtin_format_from_str(normalized.as_str()) {
            return Ok(format);
        }
        Self::custom(normalized)
    }
}

impl CustomFormatName {
    /// Creates a custom format name from user input.
    pub fn new(id: impl AsRef<str>) -> Result<Self, FormatError> {
        let normalized = id.as_ref().to_ascii_lowercase();
        validate_format_id(normalized.as_str())?;
        Ok(Self(normalized.into_boxed_str()))
    }

    /// Returns this name as a string slice.
    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

impl fmt::Display for CustomFormatName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl AsRef<str> for CustomFormatName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

fn builtin_format_from_str(s: &str) -> Option<Format> {
    match s {
        #[cfg(feature = "format-json")]
        "json" => Some(Format::Json),
        #[cfg(feature = "format-json5")]
        "json5" => Some(Format::Json5),
        "rhai" => Some(Format::Rhai),
        #[cfg(feature = "format-toml")]
        "toml" => Some(Format::Toml),
        #[cfg(feature = "format-yaml")]
        "yaml" => Some(Format::Yaml),
        _ => None,
    }
}

/// Describes where a format id is being used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatUsage {
    Input,
    Output,
}

impl fmt::Display for FormatUsage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatUsage::Input => write!(f, "input"),
            FormatUsage::Output => write!(f, "output"),
        }
    }
}

/// Unified error type for format id validation and registry resolution.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// Identifier was empty.
    #[error("format id cannot be empty")]
    EmptyFormatId,

    /// Identifier contained an invalid character.
    #[error("invalid character '{ch}' at index {index} in format id '{id}'")]
    InvalidFormatIdChar { id: String, index: usize, ch: char },

    /// Custom format id attempted to use a reserved built-in name.
    #[error("custom format id '{id}' collides with a reserved built-in format name")]
    ReservedBuiltinFormatName { id: String },

    /// Another parser/writer is already registered for this format id.
    #[error("format id '{format}' is already registered")]
    FormatIdCollision { format: Format },

    /// Another parser is already registered for this extension.
    #[error("file extension '.{extension}' is already registered")]
    ExtensionCollision { extension: String },

    /// Extension was invalid after normalization.
    #[error("invalid file extension '{extension}'")]
    InvalidExtension { extension: String },

    /// Config file path does not include an extension.
    #[error("cannot determine config format: file has no extension: {}", path.display())]
    MissingFileExtension { path: PathBuf },

    /// Config file extension is not known to the active parser registry.
    #[error(
        "unknown file extension '.{extension}' for file: {}{}",
        path.display(),
        fmt_supported_list(supported)
    )]
    UnknownFileExtension {
        path: PathBuf,
        extension: String,
        supported: Vec<String>,
    },

    /// Format id is not known to the active parser/writer registry.
    #[error(
        "unknown {usage} format '{format_id}'{}",
        fmt_supported_list(supported)
    )]
    UnknownFormatId {
        usage: FormatUsage,
        format_id: String,
        supported: Vec<String>,
    },
}

impl FormatError {
    /// Config file path is missing an extension.
    pub fn missing_file_extension(path: &Path) -> Self {
        Self::MissingFileExtension {
            path: path.to_path_buf(),
        }
    }

    /// Config file extension is not recognized by the active registry.
    pub fn unknown_file_extension(path: &Path, extension: &str, supported: &[&str]) -> Self {
        Self::UnknownFileExtension {
            path: path.to_path_buf(),
            extension: extension.to_string(),
            supported: supported.iter().map(|item| (*item).to_string()).collect(),
        }
    }

    /// Format id is not recognized by the active parser/writer registry.
    pub fn unknown_format_id(usage: FormatUsage, format_id: &str, supported: &[&str]) -> Self {
        Self::UnknownFormatId {
            usage,
            format_id: format_id.to_string(),
            supported: supported.iter().map(|item| (*item).to_string()).collect(),
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Parser and Writer Traits

/// Parses config text/files into [`Node`] values for a specific format.
pub trait ConfigFormatParser: 'static {
    /// Returns this parser's format id.
    fn id(&self) -> Format;

    /// Returns supported file extensions (without dots).
    fn extensions(&self) -> &'static [&'static str];

    /// Parses source text into a node.
    fn parse_str(&self, source: &str) -> Result<Node, NodeError>;

    /// Parses a file path into a node.
    fn parse_file(&self, path: &Path) -> Result<Node, NodeError> {
        let source = fs_err::read_to_string(path).map_err(|e| NodeError::read_file(path, e))?;
        self.parse_str(&source)
    }
}

/// Serializes [`Node`] values into a specific output format.
pub trait ConfigFormatWriter: 'static {
    /// Returns this writer's format id.
    fn id(&self) -> Format;

    /// Serializes a node.
    fn write_node(&self, node: &Node) -> Result<String, NodeError>;
}

// ---------------------------------------------------------------------------------------------- //
// FormatParserRegistry

/// Immutable parser registry used for extension/id based format dispatch.
pub struct FormatParserRegistry {
    parsers: IndexMap<Format, Box<dyn ConfigFormatParser>>,
    by_extension: IndexMap<String, Format>,
}

impl FormatParserRegistry {
    /// Returns the parser for the given extension (case-insensitive, optional leading dot).
    pub fn parser_for_extension(&self, extension: &str) -> Option<&dyn ConfigFormatParser> {
        let key = normalize_extension(extension)?;
        let format_id = self.by_extension.get(key.as_str())?;
        self.parser_by_id(format_id)
    }

    /// Returns the parser for the given format id.
    pub fn parser_by_id(&self, format_id: &Format) -> Option<&dyn ConfigFormatParser> {
        self.parsers.get(format_id).map(|parser| parser.as_ref())
    }

    /// Returns supported extensions in insertion order.
    pub fn supported_extensions(&self) -> Vec<&str> {
        self.by_extension.keys().map(|item| item.as_str()).collect()
    }

    /// Returns supported format ids in insertion order.
    pub fn supported_format_ids(&self) -> Vec<&Format> {
        self.parsers.keys().collect()
    }
}

/// Builder for [`FormatParserRegistry`].
pub struct FormatParserRegistryBuilder {
    parsers: IndexMap<Format, Box<dyn ConfigFormatParser>>,
    by_extension: IndexMap<String, Format>,
}

impl FormatParserRegistryBuilder {
    /// Creates an empty parser registry builder.
    pub fn new() -> Self {
        Self {
            parsers: IndexMap::new(),
            by_extension: IndexMap::new(),
        }
    }

    /// Registers a parser, failing on id/extension collisions.
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, parser: impl ConfigFormatParser) -> Result<Self, FormatError> {
        let id = parser.id();
        let extensions: Vec<String> = parser
            .extensions()
            .iter()
            .map(|ext| {
                normalize_extension(ext).ok_or_else(|| FormatError::InvalidExtension {
                    extension: ext.to_string(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        if self.parsers.contains_key(&id) {
            return Err(FormatError::FormatIdCollision { format: id });
        }

        if let Some(extension) =
            extensions.iter().find(|ext| self.by_extension.contains_key(ext.as_str())).cloned()
        {
            return Err(FormatError::ExtensionCollision { extension });
        }

        self.parsers.insert(id.clone(), Box::new(parser));
        for extension in extensions {
            self.by_extension.insert(extension, id.clone());
        }

        Ok(self)
    }

    /// Registers a parser, replacing prior entries with same id/extensions.
    pub fn add_or_replace(mut self, parser: impl ConfigFormatParser) -> Self {
        let id = parser.id();
        let extensions: Vec<String> =
            parser.extensions().iter().filter_map(|ext| normalize_extension(ext)).collect();

        self.remove_parser_and_extensions(&id);

        for extension in &extensions {
            if let Some(existing_id) = self.by_extension.get(extension.as_str()).cloned() {
                self.remove_parser_and_extensions(&existing_id);
            }
        }

        self.parsers.insert(id.clone(), Box::new(parser));
        for extension in extensions {
            self.by_extension.insert(extension, id.clone());
        }

        self
    }

    /// Finalizes the parser registry.
    pub fn build(self) -> FormatParserRegistry {
        FormatParserRegistry {
            parsers: self.parsers,
            by_extension: self.by_extension,
        }
    }

    fn remove_parser_and_extensions(&mut self, id: &Format) {
        if self.parsers.shift_remove(id).is_none() {
            return;
        }
        self.by_extension.retain(|_, existing_id| existing_id != id);
    }
}

impl Default for FormatParserRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------------------------- //
// FormatWriterRegistry

/// Immutable writer registry used for `--get-as` and generic format output.
pub struct FormatWriterRegistry {
    writers: IndexMap<Format, Box<dyn ConfigFormatWriter>>,
}

impl FormatWriterRegistry {
    /// Returns writer for the given format id.
    pub fn writer_by_id(&self, format_id: &Format) -> Option<&dyn ConfigFormatWriter> {
        self.writers.get(format_id).map(|writer| writer.as_ref())
    }

    /// Returns supported format ids in insertion order.
    pub fn supported_format_ids(&self) -> Vec<&Format> {
        self.writers.keys().collect()
    }
}

/// Builder for [`FormatWriterRegistry`].
pub struct FormatWriterRegistryBuilder {
    writers: IndexMap<Format, Box<dyn ConfigFormatWriter>>,
}

impl FormatWriterRegistryBuilder {
    /// Creates an empty writer registry builder.
    pub fn new() -> Self {
        Self {
            writers: IndexMap::new(),
        }
    }

    /// Registers a writer, failing on id collisions.
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, writer: impl ConfigFormatWriter) -> Result<Self, FormatError> {
        let id = writer.id();
        if self.writers.contains_key(&id) {
            return Err(FormatError::FormatIdCollision { format: id });
        }

        self.writers.insert(id, Box::new(writer));
        Ok(self)
    }

    /// Registers a writer, replacing prior entry with the same id.
    pub fn add_or_replace(mut self, writer: impl ConfigFormatWriter) -> Self {
        let id = writer.id();
        self.writers.insert(id, Box::new(writer));
        self
    }

    /// Finalizes the writer registry.
    pub fn build(self) -> FormatWriterRegistry {
        FormatWriterRegistry {
            writers: self.writers,
        }
    }
}

impl Default for FormatWriterRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------------------------- //
// Built-in Registries

/// Returns the default parser registry with built-in formats.
pub fn default_format_parser_registry() -> FormatParserRegistry {
    default_format_parser_registry_builder().build()
}

/// Returns a parser registry builder pre-populated with built-in parsers.
pub fn default_format_parser_registry_builder() -> FormatParserRegistryBuilder {
    let mut builder = FormatParserRegistryBuilder::new();
    #[cfg(feature = "format-json")]
    {
        builder = builder.add(JsonParser).expect("json parser registration must succeed");
    }
    #[cfg(feature = "format-json5")]
    {
        builder = builder.add(Json5Parser).expect("json5 parser registration must succeed");
    }
    builder = builder.add(RhaiParser).expect("rhai parser registration must succeed");
    #[cfg(feature = "format-toml")]
    {
        builder = builder.add(TomlParser).expect("toml parser registration must succeed");
    }
    #[cfg(feature = "format-yaml")]
    {
        builder = builder.add(YamlParser).expect("yaml parser registration must succeed");
    }
    builder
}

/// Returns the default writer registry with built-in formats.
pub fn default_format_writer_registry() -> FormatWriterRegistry {
    let mut builder = FormatWriterRegistryBuilder::new();
    #[cfg(feature = "format-json")]
    {
        builder = builder.add(JsonWriter).expect("json writer registration must succeed");
    }
    builder = builder.add(RhaiWriter).expect("rhai writer registration must succeed");
    #[cfg(feature = "format-toml")]
    {
        builder = builder.add(TomlWriter).expect("toml writer registration must succeed");
    }
    #[cfg(feature = "format-yaml")]
    {
        builder = builder.add(YamlWriter).expect("yaml writer registration must succeed");
    }
    builder.build()
}

// ---------------------------------------------------------------------------------------------- //
// Built-in Parsers

#[cfg(feature = "format-json")]
#[derive(Debug, Clone, Copy)]
struct JsonParser;

#[cfg(feature = "format-json")]
impl ConfigFormatParser for JsonParser {
    fn id(&self) -> Format {
        Format::Json
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["json"]
    }

    fn parse_str(&self, source: &str) -> Result<Node, NodeError> {
        let value: serde_json::Value =
            serde_json::from_str(source).map_err(|e| NodeError::parse_format("JSON", e))?;
        from_json_value(value)
    }
}

#[cfg(feature = "format-json5")]
#[derive(Debug, Clone, Copy)]
struct Json5Parser;

#[cfg(feature = "format-json5")]
impl ConfigFormatParser for Json5Parser {
    fn id(&self) -> Format {
        Format::Json5
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["json5"]
    }

    fn parse_str(&self, source: &str) -> Result<Node, NodeError> {
        let value: serde_json::Value =
            json5::from_str(source).map_err(|e| NodeError::parse_format("JSON5", e))?;
        from_json_value(value)
    }
}

#[derive(Debug, Clone, Copy)]
struct RhaiParser;

impl ConfigFormatParser for RhaiParser {
    fn id(&self) -> Format {
        Format::Rhai
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rhai"]
    }

    fn parse_str(&self, source: &str) -> Result<Node, NodeError> {
        let engine = rhai::Engine::new();
        let dynamic = engine
            .eval::<Dynamic>(source)
            .map_err(|e| NodeError::parse_format_boxed("Rhai", display_to_boxed(*e)))?;
        from_rhai_dynamic(dynamic)
    }

    fn parse_file(&self, path: &Path) -> Result<Node, NodeError> {
        let dyn_val = crate::rhai::eval_script(path)?;
        Node::from_rhai_dynamic(dyn_val)
    }
}

#[cfg(feature = "format-toml")]
#[derive(Debug, Clone, Copy)]
struct TomlParser;

#[cfg(feature = "format-toml")]
impl ConfigFormatParser for TomlParser {
    fn id(&self) -> Format {
        Format::Toml
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["toml"]
    }

    fn parse_str(&self, source: &str) -> Result<Node, NodeError> {
        let value: toml::Value =
            toml::from_str(source).map_err(|e| NodeError::parse_format("TOML", e))?;
        from_toml_value(value)
    }
}

#[cfg(feature = "format-yaml")]
#[derive(Debug, Clone, Copy)]
struct YamlParser;

#[cfg(feature = "format-yaml")]
impl ConfigFormatParser for YamlParser {
    fn id(&self) -> Format {
        Format::Yaml
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["yaml", "yml"]
    }

    fn parse_str(&self, source: &str) -> Result<Node, NodeError> {
        let value: serde_yaml::Value =
            serde_yaml::from_str(source).map_err(|e| NodeError::parse_format("YAML", e))?;
        from_yaml_value(value)
    }
}

// ---------------------------------------------------------------------------------------------- //
// Built-in Writers

#[cfg(feature = "format-json")]
#[derive(Debug, Clone, Copy)]
struct JsonWriter;

#[cfg(feature = "format-json")]
impl ConfigFormatWriter for JsonWriter {
    fn id(&self) -> Format {
        Format::Json
    }

    fn write_node(&self, node: &Node) -> Result<String, NodeError> {
        to_json_string(node)
    }
}

#[derive(Debug, Clone, Copy)]
struct RhaiWriter;

impl ConfigFormatWriter for RhaiWriter {
    fn id(&self) -> Format {
        Format::Rhai
    }

    fn write_node(&self, node: &Node) -> Result<String, NodeError> {
        to_rhai_string(node)
    }
}

#[cfg(feature = "format-toml")]
#[derive(Debug, Clone, Copy)]
struct TomlWriter;

#[cfg(feature = "format-toml")]
impl ConfigFormatWriter for TomlWriter {
    fn id(&self) -> Format {
        Format::Toml
    }

    fn write_node(&self, node: &Node) -> Result<String, NodeError> {
        to_toml_string(node)
    }
}

#[cfg(feature = "format-yaml")]
#[derive(Debug, Clone, Copy)]
struct YamlWriter;

#[cfg(feature = "format-yaml")]
impl ConfigFormatWriter for YamlWriter {
    fn id(&self) -> Format {
        Format::Yaml
    }

    fn write_node(&self, node: &Node) -> Result<String, NodeError> {
        to_yaml_string(node)
    }
}

// ---------------------------------------------------------------------------------------------- //
// Conversion from external types

/// Creates a node tree from a Rhai Dynamic value.
///
/// Use this with custom Rhai engines. For simple cases, use
/// [`Node::parse_str_as`](crate::node::Node::parse_str_as) with `Format::Rhai`,
/// or [`Node::parse_file`](crate::node::Node::parse_file).
pub fn from_rhai_dynamic(dynamic: Dynamic) -> Result<Node, NodeError> {
    from_rhai_dynamic_recursive(KeyPath::new(), dynamic)
}

fn from_rhai_dynamic_recursive(path: KeyPath, dynamic: Dynamic) -> Result<Node, NodeError> {
    if dynamic.is_unit() {
        return Ok(Node::new_leaf(path, Value::Null));
    }
    if dynamic.is_bool() {
        let value = dynamic.try_cast::<bool>().unwrap();
        return Ok(Node::new_leaf(path, Value::Bool(value)));
    }
    if dynamic.is_string() {
        let value = dynamic.try_cast::<String>().unwrap();
        return Ok(Node::new_leaf(path, Value::String(value)));
    }
    if dynamic.is_int() {
        let value = dynamic.try_cast::<i64>().unwrap();
        return Ok(Node::new_leaf(path, Value::I64(value)));
    }
    if dynamic.is_float() {
        let value = dynamic.try_cast::<f64>().unwrap();
        return Ok(Node::new_leaf(path, Value::F64(value)));
    }
    if dynamic.is_array() {
        let vec: Vec<Dynamic> = dynamic.try_cast().unwrap();
        let mut children = Vec::with_capacity(vec.len());
        for (idx, item) in vec.into_iter().enumerate() {
            let child = from_rhai_dynamic_recursive(path.clone().push_index(idx), item)?;
            children.push(child);
        }
        return Ok(Node::new_vec(path, children));
    }
    if dynamic.is_map() {
        let map: rhai::Map = dynamic.try_cast().unwrap();
        let mut children = IndexMap::new();
        for (key, value) in map {
            let child = from_rhai_dynamic_recursive(path.clone().push_key(key.to_string()), value)?;
            children.insert(key.to_string(), child);
        }
        return Ok(Node::new_map(path, children));
    }

    Err(NodeError::invalid_value(&path, "unsupported Rhai type"))
}

/// Creates a node tree from a serde_json Value.
#[cfg(feature = "format-json")]
pub fn from_json_value(value: serde_json::Value) -> Result<Node, NodeError> {
    from_json_value_recursive(KeyPath::new(), value)
}

#[cfg(feature = "format-json")]
fn from_json_value_recursive(path: KeyPath, value: serde_json::Value) -> Result<Node, NodeError> {
    match value {
        serde_json::Value::Null => Ok(Node::new_leaf(path, Value::Null)),
        serde_json::Value::Bool(b) => Ok(Node::new_leaf(path, Value::Bool(b))),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Node::new_leaf(path, Value::I64(i)))
            } else if let Some(u) = n.as_u64() {
                Ok(Node::new_leaf(path, Value::U64(u)))
            } else if let Some(f) = n.as_f64() {
                Ok(Node::new_leaf(path, Value::F64(f)))
            } else {
                Err(NodeError::invalid_value(&path, "unsupported number type"))
            }
        }
        serde_json::Value::String(s) => Ok(Node::new_leaf(path, Value::String(s))),
        serde_json::Value::Array(arr) => {
            let mut children = Vec::with_capacity(arr.len());
            for (idx, item) in arr.into_iter().enumerate() {
                let child = from_json_value_recursive(path.clone().push_index(idx), item)?;
                children.push(child);
            }
            Ok(Node::new_vec(path, children))
        }
        serde_json::Value::Object(obj) => {
            let mut children = IndexMap::new();
            for (key, val) in obj {
                let child = from_json_value_recursive(path.clone().push_key(&key), val)?;
                children.insert(key, child);
            }
            Ok(Node::new_map(path, children))
        }
    }
}

/// Creates a node tree from a TOML Value.
#[cfg(feature = "format-toml")]
pub fn from_toml_value(value: toml::Value) -> Result<Node, NodeError> {
    from_toml_value_recursive(KeyPath::new(), value)
}

#[cfg(feature = "format-toml")]
fn from_toml_value_recursive(path: KeyPath, value: toml::Value) -> Result<Node, NodeError> {
    match value {
        toml::Value::String(s) => Ok(Node::new_leaf(path, Value::String(s))),
        toml::Value::Integer(n) => Ok(Node::new_leaf(path, Value::I64(n))),
        toml::Value::Float(n) => Ok(Node::new_leaf(path, Value::F64(n))),
        toml::Value::Boolean(b) => Ok(Node::new_leaf(path, Value::Bool(b))),
        toml::Value::Datetime(dt) => Ok(Node::new_leaf(path, Value::String(dt.to_string()))),
        toml::Value::Array(arr) => {
            let mut children = Vec::with_capacity(arr.len());
            for (idx, item) in arr.into_iter().enumerate() {
                let child = from_toml_value_recursive(path.clone().push_index(idx), item)?;
                children.push(child);
            }
            Ok(Node::new_vec(path, children))
        }
        toml::Value::Table(table) => {
            let mut children = IndexMap::new();
            for (key, value) in table {
                let child = from_toml_value_recursive(path.clone().push_key(&key), value)?;
                children.insert(key, child);
            }
            Ok(Node::new_map(path, children))
        }
    }
}

/// Creates a node tree from a YAML value.
#[cfg(feature = "format-yaml")]
pub fn from_yaml_value(value: serde_yaml::Value) -> Result<Node, NodeError> {
    from_yaml_value_recursive(KeyPath::new(), value)
}

#[cfg(feature = "format-yaml")]
fn from_yaml_value_recursive(path: KeyPath, value: serde_yaml::Value) -> Result<Node, NodeError> {
    match value {
        serde_yaml::Value::Null => Ok(Node::new_leaf(path, Value::Null)),
        serde_yaml::Value::Bool(b) => Ok(Node::new_leaf(path, Value::Bool(b))),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Node::new_leaf(path, Value::I64(i)))
            } else if let Some(u) = n.as_u64() {
                Ok(Node::new_leaf(path, Value::U64(u)))
            } else if let Some(f) = n.as_f64() {
                Ok(Node::new_leaf(path, Value::F64(f)))
            } else {
                Err(NodeError::invalid_value(&path, "unsupported YAML number type"))
            }
        }
        serde_yaml::Value::String(s) => Ok(Node::new_leaf(path, Value::String(s))),
        serde_yaml::Value::Sequence(seq) => {
            let mut children = Vec::with_capacity(seq.len());
            for (idx, item) in seq.into_iter().enumerate() {
                let child = from_yaml_value_recursive(path.clone().push_index(idx), item)?;
                children.push(child);
            }
            Ok(Node::new_vec(path, children))
        }
        serde_yaml::Value::Mapping(map) => {
            let mut children = IndexMap::new();
            for (key, value) in map {
                let Some(key) = key.as_str() else {
                    return Err(NodeError::invalid_value(&path, "YAML map keys must be strings"));
                };
                let child = from_yaml_value_recursive(path.clone().push_key(key), value)?;
                children.insert(key.to_string(), child);
            }
            Ok(Node::new_map(path, children))
        }
        serde_yaml::Value::Tagged(tagged) => from_yaml_value_recursive(path, tagged.value),
    }
}

// ---------------------------------------------------------------------------------------------- //
// Conversion to external types

/// Converts a node tree to a Rhai Dynamic value.
pub fn to_rhai_dynamic(node: &Node) -> Dynamic {
    match &node.kind {
        Kind::Leaf(leaf) => match &leaf.value {
            Value::Null => Dynamic::UNIT,
            Value::Bool(b) => Dynamic::from(*b),
            Value::String(s) => Dynamic::from(s.clone()),
            Value::I8(n) => Dynamic::from(*n as i64),
            Value::I16(n) => Dynamic::from(*n as i64),
            Value::I32(n) => Dynamic::from(*n as i64),
            Value::I64(n) => Dynamic::from(*n),
            Value::U8(n) => Dynamic::from(*n as i64),
            Value::U16(n) => Dynamic::from(*n as i64),
            Value::U32(n) => Dynamic::from(*n as i64),
            Value::U64(n) => {
                if *n > i64::MAX as u64 {
                    Dynamic::from(n.to_string())
                } else {
                    Dynamic::from(*n as i64)
                }
            }
            Value::F32(n) => Dynamic::from(*n as f64),
            Value::F64(n) => Dynamic::from(*n),
        },
        Kind::Vec(vec) => {
            let arr: Vec<Dynamic> = vec.iter().map(to_rhai_dynamic).collect();
            Dynamic::from(arr)
        }
        Kind::Map(map) => {
            let mut rhai_map = rhai::Map::new();
            for (key, value) in map.iter() {
                rhai_map.insert(key.clone().into(), to_rhai_dynamic(value));
            }
            Dynamic::from(rhai_map)
        }
    }
}

/// Converts a node tree to a serde_json Value.
#[cfg(feature = "format-json")]
pub fn to_json_value(node: &Node) -> serde_json::Value {
    match &node.kind {
        Kind::Leaf(leaf) => match &leaf.value {
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => serde_json::Value::Bool(*b),
            Value::String(s) => serde_json::Value::String(s.clone()),
            Value::I8(n) => serde_json::Value::Number((*n as i64).into()),
            Value::I16(n) => serde_json::Value::Number((*n as i64).into()),
            Value::I32(n) => serde_json::Value::Number((*n as i64).into()),
            Value::I64(n) => serde_json::Value::Number((*n).into()),
            Value::U8(n) => serde_json::Value::Number((*n as u64).into()),
            Value::U16(n) => serde_json::Value::Number((*n as u64).into()),
            Value::U32(n) => serde_json::Value::Number((*n as u64).into()),
            Value::U64(n) => serde_json::Value::Number((*n).into()),
            Value::F32(n) => serde_json::Number::from_f64(*n as f64)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Value::F64(n) => serde_json::Number::from_f64(*n)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
        },
        Kind::Vec(vec) => serde_json::Value::Array(vec.iter().map(to_json_value).collect()),
        Kind::Map(map) => {
            let obj: serde_json::Map<String, serde_json::Value> =
                map.iter().map(|(k, v)| (k.clone(), to_json_value(v))).collect();
            serde_json::Value::Object(obj)
        }
    }
}

/// Converts a node tree to a pretty-printed JSON string.
#[cfg(feature = "format-json")]
fn to_json_string(node: &Node) -> Result<String, NodeError> {
    let value = to_json_value(node);
    serde_json::to_string_pretty(&value).map_err(|e| NodeError::serialize_format("JSON", e))
}

/// Converts a node tree to a pretty-printed Rhai string.
fn to_rhai_string(node: &Node) -> Result<String, NodeError> {
    let mut writer = crate::writer::RhaiWriter::to_string_writer();
    crate::writer::ToRhai::to_rhai(node, &mut writer)?;
    writer.into_string()
}

/// Converts a node tree to a pretty-printed TOML string.
#[cfg(feature = "format-toml")]
fn to_toml_string(node: &Node) -> Result<String, NodeError> {
    let value = to_toml_value(node)?;
    toml::to_string_pretty(&value).map_err(|e| NodeError::serialize_format("TOML", e))
}

/// Converts a node tree to a TOML value.
#[cfg(feature = "format-toml")]
pub fn to_toml_value(node: &Node) -> Result<toml::Value, NodeError> {
    to_toml_value_recursive(node)
}

#[cfg(feature = "format-toml")]
fn to_toml_value_recursive(node: &Node) -> Result<toml::Value, NodeError> {
    match &node.kind {
        Kind::Leaf(leaf) => match &leaf.value {
            Value::Null => {
                Err(NodeError::invalid_value(&node.path, "cannot serialize null as TOML value"))
            }
            Value::Bool(v) => Ok(toml::Value::Boolean(*v)),
            Value::String(v) => Ok(toml::Value::String(v.clone())),
            Value::I8(v) => Ok(toml::Value::Integer(*v as i64)),
            Value::I16(v) => Ok(toml::Value::Integer(*v as i64)),
            Value::I32(v) => Ok(toml::Value::Integer(*v as i64)),
            Value::I64(v) => Ok(toml::Value::Integer(*v)),
            Value::U8(v) => Ok(toml::Value::Integer(*v as i64)),
            Value::U16(v) => Ok(toml::Value::Integer(*v as i64)),
            Value::U32(v) => Ok(toml::Value::Integer(*v as i64)),
            Value::U64(v) => i64::try_from(*v).map(toml::Value::Integer).map_err(|_| {
                NodeError::invalid_value(
                    &node.path,
                    format!("u64 value {v} is too large for TOML integer (i64)"),
                )
            }),
            Value::F32(v) => Ok(toml::Value::Float(*v as f64)),
            Value::F64(v) => Ok(toml::Value::Float(*v)),
        },
        Kind::Vec(vec) => {
            let mut arr = Vec::with_capacity(vec.len());
            for child in vec {
                arr.push(to_toml_value_recursive(child)?);
            }
            Ok(toml::Value::Array(arr))
        }
        Kind::Map(map) => {
            let mut table = toml::map::Map::new();
            for (key, child) in map {
                table.insert(key.clone(), to_toml_value_recursive(child)?);
            }
            Ok(toml::Value::Table(table))
        }
    }
}

/// Converts a node tree to a YAML string.
#[cfg(feature = "format-yaml")]
fn to_yaml_string(node: &Node) -> Result<String, NodeError> {
    let value = to_yaml_value(node)?;
    serde_yaml::to_string(&value).map_err(|e| NodeError::serialize_format("YAML", e))
}

/// Converts a node tree to a YAML value.
#[cfg(feature = "format-yaml")]
pub fn to_yaml_value(node: &Node) -> Result<serde_yaml::Value, NodeError> {
    to_yaml_value_recursive(node)
}

#[cfg(feature = "format-yaml")]
fn to_yaml_value_recursive(node: &Node) -> Result<serde_yaml::Value, NodeError> {
    match &node.kind {
        Kind::Leaf(leaf) => match &leaf.value {
            Value::Null => Ok(serde_yaml::Value::Null),
            Value::Bool(v) => Ok(serde_yaml::Value::Bool(*v)),
            Value::String(v) => Ok(serde_yaml::Value::String(v.clone())),
            Value::I8(v) => yaml_number_from(*v),
            Value::I16(v) => yaml_number_from(*v),
            Value::I32(v) => yaml_number_from(*v),
            Value::I64(v) => yaml_number_from(*v),
            Value::U8(v) => yaml_number_from(*v),
            Value::U16(v) => yaml_number_from(*v),
            Value::U32(v) => yaml_number_from(*v),
            Value::U64(v) => yaml_number_from(*v),
            Value::F32(v) => yaml_number_from(*v),
            Value::F64(v) => yaml_number_from(*v),
        },
        Kind::Vec(vec) => {
            let mut seq = Vec::with_capacity(vec.len());
            for child in vec {
                seq.push(to_yaml_value_recursive(child)?);
            }
            Ok(serde_yaml::Value::Sequence(seq))
        }
        Kind::Map(map) => {
            let mut yaml_map = serde_yaml::Mapping::new();
            for (key, child) in map {
                yaml_map.insert(
                    serde_yaml::Value::String(key.clone()),
                    to_yaml_value_recursive(child)?,
                );
            }
            Ok(serde_yaml::Value::Mapping(yaml_map))
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Helpers

fn normalize_extension(extension: &str) -> Option<String> {
    let trimmed = extension.strip_prefix('.').unwrap_or(extension).to_ascii_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed)
}

fn validate_format_id(id: &str) -> Result<(), FormatError> {
    if id.is_empty() {
        return Err(FormatError::EmptyFormatId);
    }

    for (index, ch) in id.char_indices() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' {
            continue;
        }
        return Err(FormatError::InvalidFormatIdChar {
            id: id.to_string(),
            index,
            ch,
        });
    }

    Ok(())
}

fn fmt_supported_list(items: &[String]) -> String {
    if items.is_empty() {
        String::new()
    } else {
        format!(" (supported: {})", items.join(", "))
    }
}

#[cfg(feature = "format-yaml")]
fn yaml_number_from<T: serde::Serialize>(value: T) -> Result<serde_yaml::Value, NodeError> {
    serde_yaml::to_value(value).map_err(|e| NodeError::serialize_format("YAML", e))
}

// ---------------------------------------------------------------------------------------------- //
// Tests

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------------------------ //
    // Format Tests

    #[cfg(feature = "format-json")]
    #[test]
    fn format_new_maps_builtin_from_uppercase() {
        let format = Format::new("JSON").unwrap();
        assert_eq!(format, Format::Json);
    }

    #[test]
    fn format_new_rejects_empty() {
        let err = Format::new("").unwrap_err();
        assert_eq!(err, FormatError::EmptyFormatId);
    }

    #[test]
    fn format_new_rejects_underscore() {
        let err = Format::new("json_5").unwrap_err();
        let FormatError::InvalidFormatIdChar { index, ch, .. } = err else {
            panic!("expected invalid char")
        };
        assert_eq!(index, 4);
        assert_eq!(ch, '_');
    }

    #[test]
    fn format_parse_accepts_hyphen_custom() {
        let format: Format = "my-format-2".parse().unwrap();
        assert_eq!(format.as_str(), "my-format-2");
    }

    #[test]
    fn format_builtin_variants_match_expected_values() {
        #[cfg(feature = "format-json")]
        assert_eq!(Format::Json.as_str(), "json");
        #[cfg(feature = "format-json5")]
        assert_eq!(Format::Json5.as_str(), "json5");
        assert_eq!(Format::Rhai.as_str(), "rhai");
        #[cfg(feature = "format-toml")]
        assert_eq!(Format::Toml.as_str(), "toml");
        #[cfg(feature = "format-yaml")]
        assert_eq!(Format::Yaml.as_str(), "yaml");
    }

    #[test]
    fn format_custom_rejects_builtin_name() {
        let err = Format::custom("rhai").unwrap_err();
        assert!(matches!(err, FormatError::ReservedBuiltinFormatName { .. }));
    }

    // ------------------------------------------------------------------------------------------ //
    // Registry Tests

    #[derive(Debug, Clone, Copy)]
    struct DummyParser;

    impl ConfigFormatParser for DummyParser {
        fn id(&self) -> Format {
            Format::new("dummy").unwrap()
        }

        fn extensions(&self) -> &'static [&'static str] {
            &["dummy"]
        }

        fn parse_str(&self, _source: &str) -> Result<Node, NodeError> {
            Ok(Node::new())
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct DummyWriter;

    impl ConfigFormatWriter for DummyWriter {
        fn id(&self) -> Format {
            Format::new("dummy").unwrap()
        }

        fn write_node(&self, _node: &Node) -> Result<String, NodeError> {
            Ok("dummy".to_string())
        }
    }

    #[test]
    fn parser_registry_lookup_by_extension() {
        let registry = FormatParserRegistryBuilder::new().add(DummyParser).unwrap().build();
        assert!(registry.parser_for_extension("dummy").is_some());
        assert!(registry.parser_for_extension(".dummy").is_some());
        assert!(registry.parser_for_extension("missing").is_none());
    }

    #[test]
    fn parser_registry_rejects_colliding_extension() {
        #[derive(Debug, Clone, Copy)]
        struct CollidingParser;

        impl ConfigFormatParser for CollidingParser {
            fn id(&self) -> Format {
                Format::new("other").unwrap()
            }

            fn extensions(&self) -> &'static [&'static str] {
                &["dummy"]
            }

            fn parse_str(&self, _source: &str) -> Result<Node, NodeError> {
                Ok(Node::new())
            }
        }

        let result =
            FormatParserRegistryBuilder::new().add(DummyParser).unwrap().add(CollidingParser);
        let Err(err) = result else {
            panic!("expected extension collision")
        };
        assert!(matches!(err, FormatError::ExtensionCollision { .. }));
    }

    #[test]
    fn writer_registry_lookup_by_id() {
        let registry = FormatWriterRegistryBuilder::new().add(DummyWriter).unwrap().build();
        let id = Format::new("dummy").unwrap();
        assert!(registry.writer_by_id(&id).is_some());
    }

    // ------------------------------------------------------------------------------------------ //
    // Conversion Tests

    #[cfg(feature = "format-json")]
    #[test]
    fn from_json_parses_u64_max() {
        let json = serde_json::json!(u64::MAX);
        let node = from_json_value(json).unwrap();
        let Kind::Leaf(leaf) = &node.kind else {
            panic!("expected leaf")
        };
        let Value::U64(n) = leaf.value else {
            panic!("expected U64")
        };
        assert_eq!(n, u64::MAX);
    }

    #[cfg(feature = "format-json")]
    #[test]
    fn from_json_parses_i64_max_plus_one_as_u64() {
        let value = i64::MAX as u64 + 1;
        let json = serde_json::json!(value);
        let node = from_json_value(json).unwrap();
        let Kind::Leaf(leaf) = &node.kind else {
            panic!("expected leaf")
        };
        let Value::U64(n) = leaf.value else {
            panic!("expected U64")
        };
        assert_eq!(n, value);
    }

    #[cfg(feature = "format-json")]
    #[test]
    fn from_json_parses_i64_max_as_i64() {
        let json = serde_json::json!(i64::MAX);
        let node = from_json_value(json).unwrap();
        let Kind::Leaf(leaf) = &node.kind else {
            panic!("expected leaf")
        };
        let Value::I64(n) = leaf.value else {
            panic!("expected I64")
        };
        assert_eq!(n, i64::MAX);
    }

    #[cfg(feature = "format-json")]
    #[test]
    fn from_json_parses_negative_as_i64() {
        let json = serde_json::json!(-42);
        let node = from_json_value(json).unwrap();
        let Kind::Leaf(leaf) = &node.kind else {
            panic!("expected leaf")
        };
        let Value::I64(n) = leaf.value else {
            panic!("expected I64")
        };
        assert_eq!(n, -42);
    }

    #[test]
    fn to_rhai_converts_small_u64_to_int() {
        let node = Node::new_leaf(KeyPath::new(), Value::U64(1000));
        let dyn_val = to_rhai_dynamic(&node);
        assert!(dyn_val.is_int());
        assert_eq!(dyn_val.as_int().unwrap(), 1000);
    }

    #[test]
    fn to_rhai_converts_large_u64_to_string() {
        let node = Node::new_leaf(KeyPath::new(), Value::U64(u64::MAX));
        let dyn_val = to_rhai_dynamic(&node);
        assert!(dyn_val.is_string());
        assert_eq!(dyn_val.into_string().unwrap(), u64::MAX.to_string());
    }

    #[test]
    fn to_rhai_converts_i64_max_plus_one_to_string() {
        let value = i64::MAX as u64 + 1;
        let node = Node::new_leaf(KeyPath::new(), Value::U64(value));
        let dyn_val = to_rhai_dynamic(&node);
        assert!(dyn_val.is_string());
        assert_eq!(dyn_val.into_string().unwrap(), value.to_string());
    }

    #[test]
    fn to_rhai_converts_i64_max_as_int() {
        let node = Node::new_leaf(KeyPath::new(), Value::U64(i64::MAX as u64));
        let dyn_val = to_rhai_dynamic(&node);
        assert!(dyn_val.is_int());
        assert_eq!(dyn_val.as_int().unwrap(), i64::MAX);
    }

    #[cfg(feature = "format-json")]
    #[test]
    fn roundtrip_u64_max_through_json() {
        let json = serde_json::json!(u64::MAX);
        let node = from_json_value(json).unwrap();
        let back = to_json_value(&node);
        assert_eq!(back.as_u64(), Some(u64::MAX));
    }

    #[test]
    fn roundtrip_large_u64_through_rhai_string() {
        let value = i64::MAX as u64 + 1;
        let node = Node::new_leaf(KeyPath::new(), Value::U64(value));
        let dyn_val = to_rhai_dynamic(&node);

        assert!(dyn_val.is_string());
        let s = dyn_val.into_string().unwrap();

        let node_back = from_rhai_dynamic(Dynamic::from(s)).unwrap();
        let Kind::Leaf(leaf) = &node_back.kind else {
            panic!("expected leaf")
        };
        let Value::String(s) = &leaf.value else {
            panic!("expected String")
        };
        assert_eq!(s, &value.to_string());
    }

    #[cfg(feature = "format-toml")]
    #[test]
    fn from_toml_parses_datetime_as_string() {
        let node = Node::parse_str("ts = 1979-05-27T07:32:00Z", Format::Toml).unwrap();
        assert_eq!(node.req::<String>("ts").unwrap(), "1979-05-27T07:32:00Z".to_string());
    }

    #[cfg(feature = "format-toml")]
    #[test]
    fn to_toml_rejects_null() {
        let node = Node::new_leaf(KeyPath::new(), Value::Null);
        let err = to_toml_string(&node).unwrap_err();
        assert!(err.to_string().contains("cannot serialize null as TOML value"));
    }

    #[cfg(feature = "format-yaml")]
    #[test]
    fn from_yaml_parses_map() {
        let node = Node::parse_str("name: demo\ncount: 3\n", Format::Yaml).unwrap();
        assert_eq!(node.req::<String>("name").unwrap(), "demo");
        assert_eq!(node.req::<i64>("count").unwrap(), 3);
    }

    #[cfg(feature = "format-yaml")]
    #[test]
    fn from_yaml_rejects_non_string_map_keys() {
        let err = Node::parse_str("1: one\n", Format::Yaml).unwrap_err();
        assert!(err.to_string().contains("YAML map keys must be strings"));
    }

    #[cfg(feature = "format-yaml")]
    #[test]
    fn to_yaml_serializes_null() {
        let node = Node::new_leaf(KeyPath::new(), Value::Null);
        let yaml = to_yaml_string(&node).unwrap();
        assert_eq!(yaml.trim(), "null");
    }
}
