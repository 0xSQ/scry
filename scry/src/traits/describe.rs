//! The [`Describe`] trait and implementations for primitive types.

use std::path::PathBuf;

use crate::desc::Desc;

// ---------------------------------------------------------------------------------------------- //

/// Provides configuration description for a type.
///
/// Use `#[derive(scry::Config)]` to generate this alongside `FromNode`.
pub trait Describe {
    /// Returns the description for this type.
    fn describe() -> Desc;
}

// ---------------------------------------------------------------------------------------------- //
// Scalars

macro_rules! impl_scry_desc_scalar {
    ($($t:ty => $hint:expr),+ $(,)?) => {
        $(
            impl Describe for $t {
                fn describe() -> Desc {
                    Desc::plain($hint)
                }
            }
        )+
    };
}

impl_scry_desc_scalar!(
    f32 => "f32",
    f64 => "f64",
    i8 => "i8",
    i16 => "i16",
    i32 => "i32",
    i64 => "i64",
    isize => "isize",
    u8 => "u8",
    u16 => "u16",
    u32 => "u32",
    u64 => "u64",
    usize => "usize",
    bool => "bool",
    String => "string",
    PathBuf => "path",
);

// ---------------------------------------------------------------------------------------------- //
// Option, Vec

impl<T: Describe> Describe for Option<T> {
    fn describe() -> Desc {
        T::describe()
    }
}

impl<T: Describe> Describe for Vec<T> {
    fn describe() -> Desc {
        Desc::list(T::describe())
    }
}

// ---------------------------------------------------------------------------------------------- //
// Tuples

impl<A: Describe, B: Describe> Describe for (A, B) {
    fn describe() -> Desc {
        Desc::tuple(vec![A::describe(), B::describe()])
    }
}

impl<A: Describe, B: Describe, C: Describe> Describe for (A, B, C) {
    fn describe() -> Desc {
        Desc::tuple(vec![A::describe(), B::describe(), C::describe()])
    }
}

impl<A: Describe, B: Describe, C: Describe, D: Describe> Describe for (A, B, C, D) {
    fn describe() -> Desc {
        Desc::tuple(vec![A::describe(), B::describe(), C::describe(), D::describe()])
    }
}

// ---------------------------------------------------------------------------------------------- //
// Arrays

impl<T: Describe, const N: usize> Describe for [T; N] {
    fn describe() -> Desc {
        Desc::list(T::describe())
    }
}
