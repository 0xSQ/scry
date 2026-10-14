//! Optional absence handling for concrete I/O results.

use std::io;

// ---------------------------------------------------------------------------------------------- //

/// Classifies only `NotFound` I/O failures as optional absence.
///
/// Adds no filesystem lookup or path context. Applies before conversion to application errors.
/// Missing ancestors can also produce `NotFound`. Later directory-iterator errors remain
/// independent of the result that opened the iterator.
pub trait IoResultExt<T> {
    /// Returns a successful value or absence while preserving every other error unchanged.
    fn not_found_ok(self) -> io::Result<Option<T>>;
}

impl<T> IoResultExt<T> for io::Result<T> {
    fn not_found_ok(self) -> io::Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

// ---------------------------------------------------------------------------------------------- //

#[cfg(test)]
mod tests;
