//! Miscellaneous utilities.
//!
//! [`PathExt`] keeps validation, lexical components, and filesystem resolution distinct.
//! Validation methods return the supplied path. Native component methods return borrowed
//! `OsStr` values, while string methods report invalid text instead of replacing it.
//!
//! ```
//! use std::path::Path;
//! use scry::util::PathExt;
//!
//! let path = Path::new("images/photo.png");
//! assert_eq!(path.require_parent()?, Path::new("images"));
//! assert_eq!(path.require_stem()?, "photo");
//! assert_eq!(path.ext_str_opt()?, Some("png"));
//! # Ok::<(), scry::util::PathError>(())
//! ```
//!
//! `canonicalize_path()` resolves an existing path and uses compatible Windows spelling
//! when possible. `simplify_path()` only borrows a compatible representation and performs
//! no filesystem lookup. When composing a future destination, retain the raw canonical
//! ancestor until the complete filename and missing directories have been appended.
//!
//! [`IoResultExt`] handles an optional I/O result without a separate existence check.
//! Only `NotFound` becomes absence. Every other error keeps its original context.
//!
//! ```no_run
//! use scry::util::IoResultExt;
//!
//! let saved = std::fs::read_to_string("settings.json").not_found_ok()?;
//! # Ok::<(), std::io::Error>(())
//! ```

mod io_result_ext;
mod path_ext;
mod path_sort;

pub use io_result_ext::IoResultExt;
pub use path_ext::{PathError, PathExt};
pub use path_sort::PathSort;
