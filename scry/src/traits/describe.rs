//! The [`Describe`] trait and implementations for primitive types.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use crate::desc::Desc;
use crate::Node;

// ---------------------------------------------------------------------------------------------- //

/// Provides configuration description for a type.
///
/// Derives delegate to each complete field type. A field without a native implementation needs
/// an explicit `#[scry(describe_with(...))]` hook or [`crate::via`] policy. Nullability describes
/// present values, while [`crate::desc::FieldDesc::optional`] describes whether a key may be omitted.
///
/// Missing description support is a compile-time error, including inside containers:
///
/// ```compile_fail,E0277
/// use scry::Describe;
///
/// struct Custom;
/// #[derive(Describe)]
/// struct Settings {
///     values: Vec<Custom>,
/// }
/// ```
///
/// Transparent wrappers require their inner type to be described too:
///
/// ```compile_fail,E0277
/// use scry::Describe;
///
/// struct Custom;
/// #[derive(Describe)]
/// struct Wrapper(Custom);
/// ```
///
/// The same requirement applies to tuple elements and enum payloads:
///
/// ```compile_fail,E0277
/// use scry::Describe;
///
/// struct Custom;
/// #[derive(Describe)]
/// struct Pair(u32, Custom);
/// ```
///
/// ```compile_fail,E0277
/// use scry::Describe;
///
/// struct Custom;
/// #[derive(Describe)]
/// enum Selection {
///     Custom(Custom),
/// }
/// ```
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
    str => "string",
    String => "string",
    Path => "path",
    PathBuf => "path",
    () => "null",
);

// ---------------------------------------------------------------------------------------------- //
// Transparent Wrappers

impl<T: Describe + ?Sized> Describe for &T {
    fn describe() -> Desc {
        T::describe()
    }
}

impl<T: Describe + ?Sized> Describe for &mut T {
    fn describe() -> Desc {
        T::describe()
    }
}

impl<T: Describe + ?Sized> Describe for Box<T> {
    fn describe() -> Desc {
        T::describe()
    }
}

impl<T: Describe + ?Sized> Describe for Rc<T> {
    fn describe() -> Desc {
        T::describe()
    }
}

impl<T: Describe + ?Sized> Describe for Arc<T> {
    fn describe() -> Desc {
        T::describe()
    }
}

// ---------------------------------------------------------------------------------------------- //
// Nullable and Sequence Values

impl<T: Describe> Describe for Option<T> {
    fn describe() -> Desc {
        T::describe().nullable()
    }
}

impl<T: Describe> Describe for Vec<T> {
    fn describe() -> Desc {
        Desc::list(T::describe())
    }
}

impl<T: Describe> Describe for [T] {
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

// ---------------------------------------------------------------------------------------------- //
// Raw Values

impl Describe for Node {
    fn describe() -> Desc {
        Desc::plain("value").nullable()
    }
}
