//! Rhai format serialization.
//!
//! Provides [`RhaiWriter`] for serializing values to Rhai syntax, and the
//! [`ToRhai`] trait for types that can be serialized.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{BufWriter, Stdout, Write};

use indexmap::{IndexMap, IndexSet};

use crate::key_path::{is_identifier, quote_string};
use crate::node::{Kind, Node, NodeError, Value};

// ---------------------------------------------------------------------------------------------- //
// RhaiWriter

/// Writer for serializing values to Rhai format.
pub struct RhaiWriter<W: Write> {
    w: W,
    config: RhaiWriterConfig,
    indent_level: u32,
    frames: Vec<Frame>,
}

impl<W: Write> RhaiWriter<W> {
    pub fn new(writer: W) -> Self {
        Self::with_config(writer, RhaiWriterConfig::default())
    }

    pub fn with_config(w: W, config: RhaiWriterConfig) -> Self {
        Self {
            w,
            config,
            indent_level: 0,
            frames: Vec::new(),
        }
    }

    /// Writes a value implementing ToRhai.
    pub fn value<V: ToRhai>(&mut self, value: V) -> Result<(), NodeError> {
        value.to_rhai(self)
    }

    /// Writes a map using a closure.
    pub fn map<F>(&mut self, f: F) -> Result<(), NodeError>
    where
        F: FnOnce(&mut RhaiMapWriter<'_, W>) -> Result<(), NodeError>,
    {
        self.begin_map()?;
        f(&mut RhaiMapWriter { rw: self })?;
        self.end_map()
    }

    /// Writes an inline map using a closure.
    pub fn map_inline<F>(&mut self, f: F) -> Result<(), NodeError>
    where
        F: FnOnce(&mut RhaiMapWriter<'_, W>) -> Result<(), NodeError>,
    {
        self.begin_map_inline()?;
        f(&mut RhaiMapWriter { rw: self })?;
        self.end_map()
    }

    /// Writes a sequence using a closure.
    pub fn seq<F>(&mut self, f: F) -> Result<(), NodeError>
    where
        F: FnOnce(&mut RhaiSeqWriter<'_, W>) -> Result<(), NodeError>,
    {
        self.begin_seq()?;
        f(&mut RhaiSeqWriter { rw: self })?;
        self.end_seq()
    }

    /// Writes an inline sequence using a closure.
    pub fn seq_inline<F>(&mut self, f: F) -> Result<(), NodeError>
    where
        F: FnOnce(&mut RhaiSeqWriter<'_, W>) -> Result<(), NodeError>,
    {
        self.begin_seq_inline()?;
        f(&mut RhaiSeqWriter { rw: self })?;
        self.end_seq()
    }

    // ------------------------------------------------------------------------------------------ //
    // Raw writing

    fn write_raw(&mut self, s: &str) -> Result<(), NodeError> {
        self.w.write_all(s.as_bytes()).map_err(io_to_serialize_error)?;
        Ok(())
    }

    fn write_key(&mut self, key: &str) -> Result<(), NodeError> {
        if is_identifier(key) && !is_rhai_reserved_identifier(key) {
            self.write_raw(key)
        } else {
            self.write_raw(&quote_string(key))
        }
    }

    fn write_unit(&mut self) -> Result<(), NodeError> {
        self.write_raw("()")
    }

    fn write_bool(&mut self, b: bool) -> Result<(), NodeError> {
        write!(self.w, "{}", b).map_err(io_to_serialize_error)?;
        Ok(())
    }

    fn write_str(&mut self, s: &str) -> Result<(), NodeError> {
        self.write_raw(&quote_string(s))
    }

    fn write_uint(&mut self, n: u64) -> Result<(), NodeError> {
        // Rhai uses i64 internally, so large u64 values must be stringified
        if n > i64::MAX as u64 {
            write!(self.w, "\"{}\"", n).map_err(io_to_serialize_error)?;
        } else {
            write!(self.w, "{}", n).map_err(io_to_serialize_error)?;
        }
        Ok(())
    }

    fn write_int(&mut self, n: i64) -> Result<(), NodeError> {
        write!(self.w, "{}", n).map_err(io_to_serialize_error)?;
        Ok(())
    }

    fn write_f32(&mut self, f: f32) -> Result<(), NodeError> {
        write!(self.w, "{}", f).map_err(io_to_serialize_error)?;
        Ok(())
    }

    fn write_f64(&mut self, f: f64) -> Result<(), NodeError> {
        write!(self.w, "{}", f).map_err(io_to_serialize_error)?;
        Ok(())
    }

    // ------------------------------------------------------------------------------------------ //
    // Container handling

    fn begin_container(&mut self, inline: bool, container_type: ContainerType) {
        self.indent_level += 1;
        self.frames.push(Frame {
            inline,
            first_item: true,
            container_type,
        });
    }

    fn end_container(&mut self, closing: &str) -> Result<(), NodeError> {
        let frame = self.frames.pop().expect("unbalanced container");
        self.indent_level -= 1;

        if frame.inline {
            // Add space before closing brace for maps only
            if matches!(frame.container_type, ContainerType::Map) {
                self.write_raw(" ")?;
            }
            self.write_raw(closing)?;
        } else {
            if !frame.first_item && self.config.trailing_commas {
                self.write_raw(",")?;
            }
            if !frame.first_item {
                self.write_raw("\n")?;
                self.write_indent()?;
            }
            self.write_raw(closing)?;
        }
        Ok(())
    }

    fn begin_map(&mut self) -> Result<(), NodeError> {
        self.write_raw("#{")?;
        self.begin_container(false, ContainerType::Map);
        Ok(())
    }

    fn begin_map_inline(&mut self) -> Result<(), NodeError> {
        self.write_raw("#{ ")?;
        self.begin_container(true, ContainerType::Map);
        Ok(())
    }

    fn end_map(&mut self) -> Result<(), NodeError> {
        self.end_container("}")
    }

    fn begin_seq(&mut self) -> Result<(), NodeError> {
        self.write_raw("[")?;
        self.begin_container(false, ContainerType::Seq);
        Ok(())
    }

    fn begin_seq_inline(&mut self) -> Result<(), NodeError> {
        self.write_raw("[")?;
        self.begin_container(true, ContainerType::Seq);
        Ok(())
    }

    fn end_seq(&mut self) -> Result<(), NodeError> {
        self.end_container("]")
    }

    fn write_separator(&mut self) -> Result<(), NodeError> {
        let frame = self.frames.last_mut().expect("no frame");

        if frame.first_item {
            frame.first_item = false;
            if !frame.inline {
                self.write_raw("\n")?;
                self.write_indent()?;
            }
        } else if frame.inline {
            self.write_raw(", ")?;
        } else {
            self.write_raw(",\n")?;
            self.write_indent()?;
        }
        Ok(())
    }

    fn write_indent(&mut self) -> Result<(), NodeError> {
        for _ in 0..self.indent_level {
            self.w.write_all(self.config.indent.as_bytes()).map_err(io_to_serialize_error)?;
        }
        Ok(())
    }
}

// Convenience constructors
impl RhaiWriter<BufWriter<Stdout>> {
    /// Creates a writer to buffered stdout.
    pub fn stdout() -> Self {
        RhaiWriter::new(BufWriter::new(std::io::stdout()))
    }
}

impl RhaiWriter<Vec<u8>> {
    /// Creates a writer to an in-memory buffer.
    pub fn to_string_writer() -> Self {
        RhaiWriter::new(Vec::new())
    }

    /// Consumes the writer and returns the buffer as a string.
    pub fn into_string(self) -> Result<String, NodeError> {
        String::from_utf8(self.w).map_err(utf8_to_serialize_error)
    }

    /// Returns the current buffer contents as a string slice.
    pub fn as_str(&self) -> Result<&str, NodeError> {
        std::str::from_utf8(&self.w).map_err(utf8_to_serialize_error)
    }
}

/// A RhaiWriter that writes to buffered stdout.
pub type StdoutRhaiWriter = RhaiWriter<BufWriter<Stdout>>;

// ---------------------------------------------------------------------------------------------- //
// RhaiWriterConfig

/// Configuration for Rhai output formatting.
#[derive(Clone)]
pub struct RhaiWriterConfig {
    pub indent: String,
    pub trailing_commas: bool,
    pub inline_seq_threshold: Option<usize>,
}

impl Default for RhaiWriterConfig {
    fn default() -> Self {
        Self {
            indent: "    ".to_string(),
            trailing_commas: true,
            inline_seq_threshold: Some(2),
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// RhaiMapWriter

/// Helper for writing map entries within a map closure.
pub struct RhaiMapWriter<'a, W: Write> {
    rw: &'a mut RhaiWriter<W>,
}

impl<'a, W: Write> RhaiMapWriter<'a, W> {
    /// Writes a key-value entry to the map.
    pub fn entry<V: ToRhai>(&mut self, key: &str, value: V) -> Result<(), NodeError> {
        self.rw.write_separator()?;
        self.rw.write_key(key)?;
        self.rw.write_raw(": ")?;
        value.to_rhai(self.rw)
    }

    /// Writes an entry with a custom value writer.
    pub fn entry_with<F>(&mut self, key: &str, f: F) -> Result<(), NodeError>
    where
        F: FnOnce(&mut RhaiWriter<W>) -> Result<(), NodeError>,
    {
        self.rw.write_separator()?;
        self.rw.write_key(key)?;
        self.rw.write_raw(": ")?;
        f(self.rw)
    }
}

// ---------------------------------------------------------------------------------------------- //
// RhaiSeqWriter

/// Helper for writing sequence elements within a seq closure.
pub struct RhaiSeqWriter<'a, W: Write> {
    rw: &'a mut RhaiWriter<W>,
}

impl<'a, W: Write> RhaiSeqWriter<'a, W> {
    /// Writes an element to the sequence.
    pub fn elem<V: ToRhai>(&mut self, value: V) -> Result<(), NodeError> {
        self.rw.write_separator()?;
        value.to_rhai(self.rw)
    }

    /// Writes an element with a custom writer.
    pub fn elem_with<F>(&mut self, f: F) -> Result<(), NodeError>
    where
        F: FnOnce(&mut RhaiWriter<W>) -> Result<(), NodeError>,
    {
        self.rw.write_separator()?;
        f(self.rw)
    }
}

// ---------------------------------------------------------------------------------------------- //
// ToRhai trait

/// Trait for types that can be serialized to Rhai format.
pub trait ToRhai {
    fn to_rhai<W: Write>(&self, w: &mut RhaiWriter<W>) -> Result<(), NodeError>;
}

// ---------------------------------------------------------------------------------------------- //
// ToRhai implementations

impl ToRhai for () {
    fn to_rhai<W: Write>(&self, w: &mut RhaiWriter<W>) -> Result<(), NodeError> {
        w.write_unit()
    }
}

impl ToRhai for bool {
    fn to_rhai<W: Write>(&self, w: &mut RhaiWriter<W>) -> Result<(), NodeError> {
        w.write_bool(*self)
    }
}

impl ToRhai for str {
    fn to_rhai<W: Write>(&self, w: &mut RhaiWriter<W>) -> Result<(), NodeError> {
        w.write_str(self)
    }
}

impl ToRhai for String {
    fn to_rhai<W: Write>(&self, w: &mut RhaiWriter<W>) -> Result<(), NodeError> {
        w.write_str(self)
    }
}

impl ToRhai for f32 {
    fn to_rhai<W: Write>(&self, w: &mut RhaiWriter<W>) -> Result<(), NodeError> {
        w.write_f32(*self)
    }
}

impl ToRhai for f64 {
    fn to_rhai<W: Write>(&self, w: &mut RhaiWriter<W>) -> Result<(), NodeError> {
        w.write_f64(*self)
    }
}

impl ToRhai for u64 {
    fn to_rhai<W: Write>(&self, w: &mut RhaiWriter<W>) -> Result<(), NodeError> {
        w.write_uint(*self)
    }
}

macro_rules! impl_torhai_for_int {
    ($($t:ty),+ $(,)?) => {
        $(
            impl ToRhai for $t {
                fn to_rhai<W: Write>(&self, w: &mut RhaiWriter<W>) -> Result<(), NodeError> {
                    w.write_int(*self as i64)
                }
            }
        )+
    };
}

impl_torhai_for_int!(i8, i16, i32, i64, isize, u8, u16, u32, usize);

impl<T: ToRhai> ToRhai for Vec<T> {
    fn to_rhai<W: Write>(&self, rw: &mut RhaiWriter<W>) -> Result<(), NodeError> {
        let should_inline =
            rw.config.inline_seq_threshold.map(|t| self.len() <= t).unwrap_or(false);

        if should_inline {
            rw.seq_inline(|sw| {
                for item in self {
                    sw.elem(item)?;
                }
                Ok(())
            })
        } else {
            rw.seq(|sw| {
                for item in self {
                    sw.elem(item)?;
                }
                Ok(())
            })
        }
    }
}

// Set types - serialize as array
macro_rules! impl_torhai_for_set {
    ($($set:ty),+ $(,)?) => {
        $(
            impl<T: ToRhai> ToRhai for $set {
                fn to_rhai<W: Write>(&self, rw: &mut RhaiWriter<W>) -> Result<(), NodeError> {
                    let should_inline = rw.config.inline_seq_threshold.map(|t| self.len() <= t).unwrap_or(false);

                    if should_inline {
                        rw.seq_inline(|sw| {
                            for item in self {
                                sw.elem(item)?;
                            }
                            Ok(())
                        })
                    } else {
                        rw.seq(|sw| {
                            for item in self {
                                sw.elem(item)?;
                            }
                            Ok(())
                        })
                    }
                }
            }
        )+
    };
}

impl_torhai_for_set!(HashSet<T>, BTreeSet<T>, IndexSet<T>);

impl<T: ToRhai> ToRhai for Option<T> {
    fn to_rhai<W: Write>(&self, w: &mut RhaiWriter<W>) -> Result<(), NodeError> {
        match self {
            Some(value) => value.to_rhai(w),
            None => w.write_unit(),
        }
    }
}

impl<T: ToRhai> ToRhai for &T {
    fn to_rhai<W: Write>(&self, w: &mut RhaiWriter<W>) -> Result<(), NodeError> {
        (*self).to_rhai(w)
    }
}

// Map implementations
macro_rules! impl_torhai_for_map {
    ($($map:ty),+ $(,)?) => {
        $(
            impl<T: ToRhai> ToRhai for $map {
                fn to_rhai<W: Write>(&self, w: &mut RhaiWriter<W>) -> Result<(), NodeError> {
                    w.map(|mw| {
                        for (key, value) in self {
                            mw.entry(key, value)?;
                        }
                        Ok(())
                    })
                }
            }
        )+
    };
}

impl_torhai_for_map!(
    HashMap<String, T>,
    BTreeMap<String, T>,
    IndexMap<String, T>,
);

impl ToRhai for Value {
    fn to_rhai<W: Write>(&self, w: &mut RhaiWriter<W>) -> Result<(), NodeError> {
        match self {
            Value::Null => w.write_unit(),
            Value::Bool(v) => w.write_bool(*v),
            Value::String(v) => w.write_str(v),
            Value::I8(v) => w.write_int(*v as i64),
            Value::I16(v) => w.write_int(*v as i64),
            Value::I32(v) => w.write_int(*v as i64),
            Value::I64(v) => w.write_int(*v),
            Value::U8(v) => w.write_uint(*v as u64),
            Value::U16(v) => w.write_uint(*v as u64),
            Value::U32(v) => w.write_uint(*v as u64),
            Value::U64(v) => w.write_uint(*v),
            Value::F32(v) => w.write_f32(*v),
            Value::F64(v) => w.write_f64(*v),
        }
    }
}

impl ToRhai for Node {
    fn to_rhai<W: Write>(&self, w: &mut RhaiWriter<W>) -> Result<(), NodeError> {
        match &self.kind {
            Kind::Leaf(leaf) => leaf.value.to_rhai(w),
            Kind::Vec(vec) => vec.to_rhai(w),
            Kind::Map(map) => map.to_rhai(w),
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Internal helpers

struct Frame {
    inline: bool,
    first_item: bool,
    container_type: ContainerType,
}

enum ContainerType {
    Map,
    Seq,
}

fn is_rhai_reserved_identifier(key: &str) -> bool {
    // Rhai map keys can be bare identifiers, but keywords and reserved identifiers need quotes.
    matches!(
        key,
        "Fn" | "as"
            | "async"
            | "await"
            | "break"
            | "call"
            | "case"
            | "catch"
            | "const"
            | "continue"
            | "curry"
            | "debug"
            | "default"
            | "do"
            | "else"
            | "eval"
            | "export"
            | "false"
            | "fn"
            | "for"
            | "go"
            | "global"
            | "goto"
            | "if"
            | "import"
            | "in"
            | "is"
            | "is_def_fn"
            | "is_def_var"
            | "is_shared"
            | "let"
            | "loop"
            | "match"
            | "module"
            | "new"
            | "nil"
            | "null"
            | "package"
            | "print"
            | "private"
            | "protected"
            | "public"
            | "return"
            | "shared"
            | "spawn"
            | "static"
            | "super"
            | "switch"
            | "sync"
            | "this"
            | "thread"
            | "throw"
            | "true"
            | "try"
            | "type_of"
            | "until"
            | "use"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
    )
}

#[cfg(test)]
pub(crate) const RHAI_RESERVED_IDENTIFIERS_FOR_TESTS: &[&str] = &[
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

fn io_to_serialize_error(err: std::io::Error) -> NodeError {
    NodeError::serialize_format("Rhai", err)
}

fn utf8_to_serialize_error(err: impl std::error::Error + Send + Sync + 'static) -> NodeError {
    NodeError::serialize_format("Rhai", err)
}
