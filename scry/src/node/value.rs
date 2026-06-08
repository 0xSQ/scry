//! Primitive values stored in leaf nodes.

use std::fmt;

// ---------------------------------------------------------------------------------------------- //
// Value

/// A primitive value stored in a leaf node.
#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    String(String),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
}

impl Value {
    /// Returns the type name of this value for error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::String(_) => "string",
            Value::I8(_) => "i8",
            Value::I16(_) => "i16",
            Value::I32(_) => "i32",
            Value::I64(_) => "i64",
            Value::U8(_) => "u8",
            Value::U16(_) => "u16",
            Value::U32(_) => "u32",
            Value::U64(_) => "u64",
            Value::F32(_) => "f32",
            Value::F64(_) => "f64",
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(v) => write!(f, "{}", v),
            Value::String(v) => write!(f, "{}", v),
            Value::I8(v) => write!(f, "{}", v),
            Value::I16(v) => write!(f, "{}", v),
            Value::I32(v) => write!(f, "{}", v),
            Value::I64(v) => write!(f, "{}", v),
            Value::U8(v) => write!(f, "{}", v),
            Value::U16(v) => write!(f, "{}", v),
            Value::U32(v) => write!(f, "{}", v),
            Value::U64(v) => write!(f, "{}", v),
            Value::F32(v) => write!(f, "{}", v),
            Value::F64(v) => write!(f, "{}", v),
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// IntoValue

/// Trait for types that can be converted into a [`Value`].
///
/// Used by [`Node::set_value`](super::Node::set_value) to accept various primitive types.
pub trait IntoValue {
    /// Converts this value into a [`Value`].
    fn into_value(self) -> Value;
}

impl IntoValue for () {
    fn into_value(self) -> Value {
        Value::Null
    }
}

impl IntoValue for bool {
    fn into_value(self) -> Value {
        Value::Bool(self)
    }
}

impl IntoValue for String {
    fn into_value(self) -> Value {
        Value::String(self)
    }
}

impl IntoValue for &str {
    fn into_value(self) -> Value {
        Value::String(self.to_owned())
    }
}

impl IntoValue for usize {
    fn into_value(self) -> Value {
        Value::U64(self as u64)
    }
}

impl IntoValue for isize {
    fn into_value(self) -> Value {
        Value::I64(self as i64)
    }
}

macro_rules! impl_into_value {
    ($($t:ty => $variant:ident),+ $(,)?) => {
        $(
            impl IntoValue for $t {
                fn into_value(self) -> Value {
                    Value::$variant(self)
                }
            }
        )+
    };
}

impl_into_value!(
    i8 => I8, i16 => I16, i32 => I32, i64 => I64,
    u8 => U8, u16 => U16, u32 => U32, u64 => U64,
    f32 => F32, f64 => F64,
);
