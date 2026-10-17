//! Renders configuration values and annotated documents as Rhai or JSON5.

use std::fmt::Write as _;
use std::io::{BufWriter, Stdout, Write};

use crate::key_path::is_identifier;
use crate::node::{Kind, Node, NodeError, Value};
use crate::{KeyPath, ToNode};

use super::document::{Annotations, Document, Layout};

// ---------------------------------------------------------------------------------------------- //

/// Renders values and documented configuration as Rhai literals.
pub struct RhaiWriter<W: Write> {
    output: W,
    config: LiteralWriterConfig,
}

impl<W: Write> RhaiWriter<W> {
    /// Creates a writer with the default formatting.
    pub fn new(output: W) -> Self {
        Self::with_config(output, LiteralWriterConfig::default())
    }

    /// Creates a writer with explicit formatting.
    pub fn with_config(output: W, config: RhaiWriterConfig) -> Self {
        Self { output, config }
    }

    /// Writes a complete document, ending with a newline.
    pub fn write(&mut self, document: &Document) -> Result<(), NodeError> {
        Renderer::new(&mut self.output, &self.config, Syntax::Rhai).document(document)
    }

    /// Writes a value without adding a final newline.
    pub fn value(&mut self, value: impl ToNode) -> Result<(), NodeError> {
        let node = value.to_node()?;
        Renderer::new(&mut self.output, &self.config, Syntax::Rhai).value(&node)
    }
}

impl RhaiWriter<BufWriter<Stdout>> {
    /// Creates a buffered standard-output writer.
    pub fn stdout() -> Self {
        Self::new(BufWriter::new(std::io::stdout()))
    }
}

impl RhaiWriter<Vec<u8>> {
    /// Creates an in-memory writer.
    pub fn to_string_writer() -> Self {
        Self::new(Vec::new())
    }

    /// Consumes the writer and returns its text.
    pub fn into_string(self) -> Result<String, NodeError> {
        String::from_utf8(self.output).map_err(|error| NodeError::serialize_format("Rhai", error))
    }

    /// Returns the text written so far.
    pub fn as_str(&self) -> Result<&str, NodeError> {
        std::str::from_utf8(&self.output)
            .map_err(|error| NodeError::serialize_format("Rhai", error))
    }
}

/// A buffered Rhai writer targeting standard output.
pub type StdoutRhaiWriter = RhaiWriter<BufWriter<Stdout>>;

/// Renders values and documented configuration as JSON5 literals.
///
/// Keys and strings use JSON quoting. Comments and optional trailing commas make the
/// documented output JSON5. Nonfinite numbers are rejected.
#[cfg(feature = "format-json5")]
pub struct Json5Writer<W: Write> {
    output: W,
    config: LiteralWriterConfig,
}

#[cfg(feature = "format-json5")]
impl<W: Write> Json5Writer<W> {
    /// Creates a writer with the default formatting.
    pub fn new(output: W) -> Self {
        Self::with_config(output, LiteralWriterConfig::default())
    }

    /// Creates a writer with explicit formatting.
    pub fn with_config(output: W, config: Json5WriterConfig) -> Self {
        Self { output, config }
    }

    /// Writes a complete document, ending with a newline.
    pub fn write(&mut self, document: &Document) -> Result<(), NodeError> {
        Renderer::new(&mut self.output, &self.config, Syntax::Json5).document(document)
    }

    /// Writes a value without adding a final newline.
    pub fn value(&mut self, value: impl ToNode) -> Result<(), NodeError> {
        let node = value.to_node()?;
        Renderer::new(&mut self.output, &self.config, Syntax::Json5).value(&node)
    }
}

#[cfg(feature = "format-json5")]
impl Json5Writer<Vec<u8>> {
    /// Creates an in-memory writer.
    pub fn to_string_writer() -> Self {
        Self::new(Vec::new())
    }

    /// Consumes the writer and returns its text.
    pub fn into_string(self) -> Result<String, NodeError> {
        String::from_utf8(self.output).map_err(|error| NodeError::serialize_format("JSON5", error))
    }

    /// Returns the text written so far.
    pub fn as_str(&self) -> Result<&str, NodeError> {
        std::str::from_utf8(&self.output)
            .map_err(|error| NodeError::serialize_format("JSON5", error))
    }
}

/// Controls formatting shared by the Rhai and JSON5 literal renderers.
#[derive(Debug, Clone)]
pub struct LiteralWriterConfig {
    /// The indentation added for each expanded container.
    pub indent: String,
    /// Whether expanded containers include a comma after their final entry.
    pub trailing_commas: bool,
    /// The maximum length of an automatically compact sequence of scalar values.
    pub inline_seq_threshold: Option<usize>,
}

impl Default for LiteralWriterConfig {
    fn default() -> Self {
        Self {
            indent: "    ".to_string(),
            trailing_commas: true,
            inline_seq_threshold: Some(2),
        }
    }
}

/// Formatting options for a Rhai writer.
pub type RhaiWriterConfig = LiteralWriterConfig;

/// Formatting options for a JSON5 writer.
#[cfg(feature = "format-json5")]
pub type Json5WriterConfig = LiteralWriterConfig;

// ---------------------------------------------------------------------------------------------- //

struct Renderer<'a, W> {
    output: &'a mut W,
    config: &'a LiteralWriterConfig,
    syntax: Syntax,
}

impl<'a, W: Write> Renderer<'a, W> {
    fn new(output: &'a mut W, config: &'a LiteralWriterConfig, syntax: Syntax) -> Self {
        Self {
            output,
            config,
            syntax,
        }
    }

    fn document(&mut self, document: &Document) -> Result<(), NodeError> {
        self.validate_config()?;
        self.validate(&document.node, &document.annotations, &document.node.path)?;
        if let Some(header) = &document.header {
            self.comments(header, 0)?;
        }
        self.leading(&document.annotations, 0)?;
        self.node(&document.node, &document.annotations, 0, false)?;
        self.end_comment(&document.annotations)?;
        self.raw("\n")?;
        if let Some(footer) = &document.footer {
            self.comments(footer, 0)?;
        }
        Ok(())
    }

    fn value(&mut self, node: &Node) -> Result<(), NodeError> {
        self.validate_config()?;
        let annotations = Annotations::default();
        self.validate(node, &annotations, &node.path)?;
        self.node(node, &annotations, 0, false)
    }

    fn validate_config(&self) -> Result<(), NodeError> {
        if !self.config.indent.chars().all(|character| matches!(character, ' ' | '\t')) {
            return Err(NodeError::new("Indentation must contain only spaces or tabs."));
        }
        Ok(())
    }

    fn validate(
        &self,
        node: &Node,
        annotations: &Annotations,
        path: &KeyPath,
    ) -> Result<(), NodeError> {
        if annotations.end_comment.as_deref().is_some_and(|text| text.contains(is_line_break)) {
            return Err(NodeError::invalid_value(path, "An end comment must fit on one line."));
        }
        match &node.kind {
            Kind::Leaf(leaf) => {
                if annotations.tail_comment.is_some() {
                    return Err(NodeError::invalid_value(
                        path,
                        "A tail comment requires a container.",
                    ));
                }
                match &leaf.value {
                    Value::U64(value) if *value > i64::MAX as u64 => {
                        return Err(NodeError::invalid_value(path, match self.syntax {
                            Syntax::Rhai => "The integer is too large for Rhai's signed 64-bit integer.",
                            #[cfg(feature = "format-json5")]
                            Syntax::Json5 => "The integer is too large for Scry's JSON5 reader's signed 64-bit integer.",
                        }));
                    }
                    Value::F32(value) if !value.is_finite() => {
                        return Err(NodeError::invalid_value(
                            path,
                            "Cannot write a nonfinite floating-point value.",
                        ));
                    }
                    Value::F64(value) if !value.is_finite() => {
                        return Err(NodeError::invalid_value(
                            path,
                            "Cannot write a nonfinite floating-point value.",
                        ));
                    }
                    _ => {}
                }
            }
            Kind::Map(map) => {
                for (index, (key, child)) in map.iter().enumerate() {
                    self.validate(child, annotations.child(index), &path.push_key(key))?;
                }
            }
            Kind::Vec(values) => {
                for (index, child) in values.iter().enumerate() {
                    self.validate(child, annotations.child(index), &path.push_index(index))?;
                }
            }
        }
        Ok(())
    }

    fn node(
        &mut self,
        node: &Node,
        annotations: &Annotations,
        depth: usize,
        compact_parent: bool,
    ) -> Result<(), NodeError> {
        match &node.kind {
            Kind::Leaf(leaf) => self.scalar(&leaf.value),
            Kind::Map(_) | Kind::Vec(_) => self.container(node, annotations, depth, compact_parent),
        }
    }

    fn container(
        &mut self,
        node: &Node,
        annotations: &Annotations,
        depth: usize,
        compact_parent: bool,
    ) -> Result<(), NodeError> {
        let (opening, closing, len, is_map) = match &node.kind {
            Kind::Map(map) => (self.syntax.map_open(), "}", map.len(), true),
            Kind::Vec(values) => ("[", "]", values.len(), false),
            Kind::Leaf(_) => unreachable!("containers have children"),
        };
        let compact = self.compact(node, annotations, compact_parent);
        self.raw(opening)?;
        if compact {
            if is_map && len > 0 {
                self.raw(" ")?;
            }
        } else {
            self.raw("\n")?;
        }
        match &node.kind {
            Kind::Map(map) => {
                for (index, (key, child)) in map.iter().enumerate() {
                    self.entry_start(annotations.child(index), depth, index, compact)?;
                    self.key(key)?;
                    self.raw(": ")?;
                    self.node(child, annotations.child(index), depth + 1, compact)?;
                    self.entry_end(annotations.child(index), index, len, compact)?;
                }
            }
            Kind::Vec(values) => {
                for (index, child) in values.iter().enumerate() {
                    self.entry_start(annotations.child(index), depth, index, compact)?;
                    self.node(child, annotations.child(index), depth + 1, compact)?;
                    self.entry_end(annotations.child(index), index, len, compact)?;
                }
            }
            Kind::Leaf(_) => unreachable!("containers have children"),
        }
        if compact {
            if is_map && len > 0 {
                self.raw(" ")?;
            }
        } else {
            if let Some(comment) = &annotations.tail_comment {
                self.comments(comment, depth + 1)?;
            }
            self.indent(depth)?;
        }
        self.raw(closing)
    }

    fn compact(&self, node: &Node, annotations: &Annotations, compact_parent: bool) -> bool {
        let (len, simple_sequence) = match &node.kind {
            Kind::Leaf(_) => return true,
            Kind::Map(map) => (map.len(), false),
            Kind::Vec(values) => {
                (values.len(), values.iter().all(|child| matches!(child.kind, Kind::Leaf(_))))
            }
        };
        if annotations.tail_comment.is_some() {
            return false;
        }
        if len == 0 {
            return true;
        }
        let requested = match annotations.layout {
            Layout::Compact => true,
            Layout::Expanded => false,
            Layout::Auto => {
                compact_parent
                    || simple_sequence
                        && self.config.inline_seq_threshold.is_some_and(|limit| len <= limit)
            }
        };
        if !requested {
            return false;
        }
        let can_compact = |(index, child): (usize, &Node)| {
            let child_annotations = annotations.child(index);
            !child_annotations.blank_before
                && child_annotations.comment.is_none()
                && child_annotations.end_comment.is_none()
                && self.compact(child, child_annotations, true)
        };
        match &node.kind {
            Kind::Map(map) => map.values().enumerate().all(can_compact),
            Kind::Vec(values) => values.iter().enumerate().all(can_compact),
            Kind::Leaf(_) => true,
        }
    }

    fn entry_start(
        &mut self,
        annotations: &Annotations,
        depth: usize,
        index: usize,
        compact: bool,
    ) -> Result<(), NodeError> {
        if compact {
            if index > 0 {
                self.raw(", ")?;
            }
        } else {
            self.leading(annotations, depth + 1)?;
            self.indent(depth + 1)?;
        }
        Ok(())
    }

    fn entry_end(
        &mut self,
        annotations: &Annotations,
        index: usize,
        len: usize,
        compact: bool,
    ) -> Result<(), NodeError> {
        if !compact {
            if index + 1 < len || self.config.trailing_commas {
                self.raw(",")?;
            }
            self.end_comment(annotations)?;
            self.raw("\n")?;
        }
        Ok(())
    }

    fn leading(&mut self, annotations: &Annotations, depth: usize) -> Result<(), NodeError> {
        if annotations.blank_before {
            self.raw("\n")?;
        }
        if let Some(comment) = &annotations.comment {
            self.comments(comment, depth)?;
        }
        Ok(())
    }

    fn end_comment(&mut self, annotations: &Annotations) -> Result<(), NodeError> {
        if let Some(comment) = &annotations.end_comment {
            self.raw(" //")?;
            if !comment.is_empty() {
                self.raw(" ")?;
                self.raw(comment)?;
            }
        }
        Ok(())
    }

    fn comments(&mut self, text: &str, depth: usize) -> Result<(), NodeError> {
        let normalized = text.replace("\r\n", "\n");
        for line in normalized.split(is_line_break) {
            self.indent(depth)?;
            self.raw("//")?;
            if !line.is_empty() {
                self.raw(" ")?;
                self.raw(line)?;
            }
            self.raw("\n")?;
        }
        Ok(())
    }

    fn key(&mut self, key: &str) -> Result<(), NodeError> {
        if matches!(self.syntax, Syntax::Rhai)
            && is_identifier(key)
            && !RHAI_RESERVED_IDENTIFIERS.contains(&key)
        {
            self.raw(key)
        } else {
            self.string(key)
        }
    }

    fn scalar(&mut self, value: &Value) -> Result<(), NodeError> {
        let text = match value {
            Value::Null => return self.raw(self.syntax.null()),
            Value::String(text) => return self.string(text),
            Value::Bool(value) => value.to_string(),
            Value::I8(value) => value.to_string(),
            Value::I16(value) => value.to_string(),
            Value::I32(value) => value.to_string(),
            Value::I64(value) => value.to_string(),
            Value::U8(value) => value.to_string(),
            Value::U16(value) => value.to_string(),
            Value::U32(value) => value.to_string(),
            Value::U64(value) => value.to_string(),
            Value::F32(value) => format!("{value:?}"),
            Value::F64(value) => format!("{value:?}"),
        };
        self.raw(&text)
    }

    fn string(&mut self, text: &str) -> Result<(), NodeError> {
        let mut quoted = String::with_capacity(text.len() + 2);
        quoted.push('"');
        for character in text.chars() {
            match character {
                '\\' => quoted.push_str("\\\\"),
                '"' => quoted.push_str("\\\""),
                '\n' => quoted.push_str("\\n"),
                '\r' => quoted.push_str("\\r"),
                '\t' => quoted.push_str("\\t"),
                character
                    if character.is_control() || matches!(character, '\u{2028}' | '\u{2029}') =>
                {
                    write!(quoted, "\\u{:04X}", character as u32)
                        .expect("writing to a String cannot fail");
                }
                character => quoted.push(character),
            }
        }
        quoted.push('"');
        self.raw(&quoted)
    }

    fn indent(&mut self, depth: usize) -> Result<(), NodeError> {
        for _ in 0..depth {
            self.raw(&self.config.indent)?;
        }
        Ok(())
    }

    fn raw(&mut self, text: &str) -> Result<(), NodeError> {
        self.output
            .write_all(text.as_bytes())
            .map_err(|error| NodeError::serialize_format(self.syntax.name(), error))
    }
}

#[derive(Clone, Copy)]
enum Syntax {
    Rhai,
    #[cfg(feature = "format-json5")]
    Json5,
}

impl Syntax {
    fn name(self) -> &'static str {
        match self {
            Self::Rhai => "Rhai",
            #[cfg(feature = "format-json5")]
            Self::Json5 => "JSON5",
        }
    }

    fn map_open(self) -> &'static str {
        match self {
            Self::Rhai => "#{",
            #[cfg(feature = "format-json5")]
            Self::Json5 => "{",
        }
    }

    fn null(self) -> &'static str {
        match self {
            Self::Rhai => "()",
            #[cfg(feature = "format-json5")]
            Self::Json5 => "null",
        }
    }
}

fn is_line_break(character: char) -> bool {
    matches!(character, '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

const RHAI_RESERVED_IDENTIFIERS: &[&str] = &[
    "Fn",
    "as",
    "async",
    "await",
    "break",
    "call",
    "case",
    "catch",
    "const",
    "continue",
    "curry",
    "debug",
    "default",
    "do",
    "else",
    "eval",
    "export",
    "false",
    "fn",
    "for",
    "go",
    "global",
    "goto",
    "if",
    "import",
    "in",
    "is",
    "is_def_fn",
    "is_def_var",
    "is_shared",
    "let",
    "loop",
    "match",
    "module",
    "new",
    "nil",
    "null",
    "package",
    "print",
    "private",
    "protected",
    "public",
    "return",
    "shared",
    "spawn",
    "static",
    "super",
    "switch",
    "sync",
    "this",
    "thread",
    "throw",
    "true",
    "try",
    "type_of",
    "until",
    "use",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

#[cfg(all(test, feature = "format-json"))]
pub(super) const RHAI_RESERVED_IDENTIFIERS_FOR_TESTS: &[&str] = RHAI_RESERVED_IDENTIFIERS;
