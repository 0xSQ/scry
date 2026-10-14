//! Path validation, native components, and compatible path representations.

use super::IoResultExt;
use std::ffi::OsStr;
use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------------- //

/// Extends paths with fallible checks, native components, and compatible representations.
///
/// Checks and directory operations preserve the supplied path for chaining. Only
/// `canonicalize_path()` resolves it through the filesystem and returns an owned absolute path.
pub trait PathExt: AsRef<Path> {
    // ------------------------------------------------------------------------------------------ //
    // Existence Checks

    /// Validates that the path has an existing filesystem target.
    ///
    /// Follows symbolic links and preserves the supplied path.
    fn require_exists(&self) -> Result<&Path, PathError> {
        let path = self.as_ref();
        if try_exists(path)? {
            Ok(path)
        } else {
            Err(PathError::NotFound {
                path: path.to_path_buf(),
            })
        }
    }

    /// Validates that the path has no existing filesystem target.
    ///
    /// Follows links, so a dangling final link passes. This does not reserve an entry or
    /// prevent another operation from creating it.
    fn require_missing(&self) -> Result<&Path, PathError> {
        let path = self.as_ref();
        if try_exists(path)? {
            Err(PathError::AlreadyExists {
                path: path.to_path_buf(),
            })
        } else {
            Ok(path)
        }
    }

    /// Validates that the path resolves to a regular file.
    fn require_file(&self) -> Result<&Path, PathError> {
        let path = self.as_ref();
        if required_metadata(path)?.is_file() {
            Ok(path)
        } else {
            Err(PathError::NotAFile {
                path: path.to_path_buf(),
            })
        }
    }

    /// Validates that the path resolves to a directory.
    fn require_dir(&self) -> Result<&Path, PathError> {
        let path = self.as_ref();
        require_directory(path, &required_metadata(path)?)
    }

    /// Validates that the parent directory exists and is a directory.
    ///
    /// Returns the original path. An absent or empty lexical parent, including the parent
    /// of a bare filename, requires no filesystem lookup.
    fn require_parent_dir(&self) -> Result<&Path, PathError> {
        let path = self.as_ref();
        check_parent_is_existing_dir(path)?;
        Ok(path)
    }

    // ------------------------------------------------------------------------------------------ //
    // Directory Operations

    /// Ensures the directory exists, creating it if needed.
    ///
    /// The parent directory must already exist. Existing directory links are followed.
    fn ensure_dir(&self) -> Result<&Path, PathError> {
        let path = self.as_ref();
        check_parent_is_existing_dir(path)?;
        match metadata_if_exists(path)? {
            Some(metadata) => require_directory(path, &metadata),
            None => create_directory(path, false),
        }
    }

    /// Ensures the directory and all parents exist.
    ///
    /// Existing directory links are followed.
    fn ensure_dir_all(&self) -> Result<&Path, PathError> {
        let path = self.as_ref();
        match metadata_if_exists(path)? {
            Some(metadata) => require_directory(path, &metadata),
            None => create_directory(path, true),
        }
    }

    /// Removes an existing directory entry, then creates it fresh.
    ///
    /// The parent directory must already exist. Validation follows directory links, while
    /// removal uses `std::fs::remove_dir_all()`. A final directory symlink can therefore be
    /// replaced with an ordinary directory without deleting the linked directory's contents.
    /// Linked ancestors are still followed. This does not establish containment.
    fn recreate_dir(&self) -> Result<&Path, PathError> {
        let path = self.as_ref();
        check_parent_is_existing_dir(path)?;
        recreate_directory(path, false)
    }

    /// Removes an existing directory entry, then creates it and all parents.
    ///
    /// Uses the same link and removal behavior as `recreate_dir()`.
    fn recreate_dir_all(&self) -> Result<&Path, PathError> {
        recreate_directory(self.as_ref(), true)
    }

    /// Ensures the parent directory exists, creating it and all ancestors if needed.
    ///
    /// An absent or empty lexical parent requires no filesystem lookup.
    fn ensure_parent_dir(&self) -> Result<&Path, PathError> {
        let path = self.as_ref();
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            parent.ensure_dir_all()?;
        }
        Ok(path)
    }

    // ------------------------------------------------------------------------------------------ //
    // Path Representations

    /// Resolves an existing path to a compatible canonical absolute path.
    ///
    /// Resolves links through the filesystem. On Windows, ordinary drive-path spelling is
    /// preferred when conversion preserves interpretation. Paths requiring verbatim syntax,
    /// and unsupported namespaces such as extended UNC, retain that syntax. Other platforms
    /// use standard canonicalization. Errors retain the original input path.
    fn canonicalize_path(&self) -> Result<PathBuf, PathError> {
        let path = self.as_ref();
        dunce::canonicalize(path).map_err(|source| PathError::Io {
            operation: "canonicalize path",
            path: path.to_path_buf(),
            source,
        })
    }

    /// Returns a borrowed path with compatible Windows spelling where possible.
    ///
    /// Performs no filesystem I/O. Relative and non-Windows paths remain unchanged, as do
    /// paths whose conversion is unsupported or would change interpretation. For a future
    /// destination, simplifies only after its complete missing suffix has been appended
    /// to the raw canonical ancestor.
    fn simplify_path(&self) -> &Path {
        dunce::simplified(self.as_ref())
    }

    // ------------------------------------------------------------------------------------------ //
    // Native Components and String Conversions

    /// Returns the required native filename without accessing the filesystem.
    fn require_file_name(&self) -> Result<&OsStr, PathError> {
        let path = self.as_ref();
        required_component(path, "file name", path.file_name())
    }

    /// Returns the required native file stem without accessing the filesystem.
    fn require_stem(&self) -> Result<&OsStr, PathError> {
        let path = self.as_ref();
        required_component(path, "file stem", path.file_stem())
    }

    /// Returns the required lexical parent without accessing the filesystem.
    ///
    /// Preserves standard parent semantics, including an empty parent for a bare filename.
    /// Does not substitute the current directory or require the parent to exist.
    fn require_parent(&self) -> Result<&Path, PathError> {
        let path = self.as_ref();
        required_component(path, "parent", path.parent())
    }

    /// Returns the full path as a string slice.
    fn path_str(&self) -> Result<&str, PathError> {
        let path = self.as_ref();
        path.to_str().ok_or_else(|| PathError::InvalidUtf8 {
            path: path.to_path_buf(),
            component: "path",
        })
    }

    /// Returns the required filename as a string slice.
    fn file_name_str(&self) -> Result<&str, PathError> {
        let path = self.as_ref();
        os_str_to_str(path, "file name", path.file_name())
    }

    /// Returns the required file stem as a string slice.
    fn stem_str(&self) -> Result<&str, PathError> {
        let path = self.as_ref();
        os_str_to_str(path, "file stem", path.file_stem())
    }

    /// Returns the required file extension as a string slice.
    fn ext_str(&self) -> Result<&str, PathError> {
        let path = self.as_ref();
        os_str_to_str(path, "file extension", path.extension())
    }

    /// Returns an optional text extension, preserving invalid UTF-8 as an error.
    ///
    /// A missing extension returns `None`. An empty extension remains `Some("")`.
    fn ext_str_opt(&self) -> Result<Option<&str>, PathError> {
        let path = self.as_ref();
        path.extension()
            .map(|extension| os_str_to_str(path, "file extension", Some(extension)))
            .transpose()
    }

    /// Checks whether the extension matches the given extension without regard to case.
    ///
    /// Missing and non-text extensions return `false`.
    fn has_ext(&self, ext: &str) -> bool {
        self.as_ref()
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case(ext))
    }

    /// Checks whether the extension matches any supplied extension without regard to case.
    ///
    /// Missing and non-text extensions return `false`.
    fn has_any_ext(&self, exts: &[&str]) -> bool {
        let Some(ext) = self.as_ref().extension().and_then(|value| value.to_str()) else {
            return false;
        };
        exts.iter().any(|value| ext.eq_ignore_ascii_case(value))
    }

    /// Checks whether the filename ends with the supplied extension suffixes.
    ///
    /// Suffixes are ordered left-to-right and matched without regard to case. Missing,
    /// non-text, or unmatched names return `false`. Empty, `.` and `..` bases are rejected.
    ///
    /// ```
    /// # use std::path::Path;
    /// # use scry::util::PathExt;
    /// assert!(Path::new("archive.tar.gz").ends_with_suffixes(&["tar", "gz"]));
    /// assert!(Path::new("archive.tar.gz").ends_with_suffixes(&["gz"]));
    /// assert!(!Path::new("archive.tar.gz").ends_with_suffixes(&["zip"]));
    /// ```
    fn ends_with_suffixes(&self, suffixes: &[&str]) -> bool {
        let Some(name) = self.as_ref().file_name().and_then(|value| value.to_str()) else {
            return false;
        };
        match_suffixes(name, suffixes).is_some()
    }

    /// Strips the supplied extension suffixes from a filename, returning a new path.
    ///
    /// Suffixes are ordered left-to-right and matched without regard to case. Missing,
    /// non-text, or unmatched names return `None`. Empty, `.` and `..` bases are rejected.
    ///
    /// ```
    /// # use std::path::Path;
    /// # use scry::util::PathExt;
    /// assert_eq!(Path::new("archive.tar.gz").strip_suffixes(&["tar", "gz"]),
    ///     Some("archive".into()));
    /// assert_eq!(Path::new("my.archive.tar.gz").strip_suffixes(&["gz"]),
    ///     Some("my.archive.tar".into()));
    /// ```
    fn strip_suffixes(&self, suffixes: &[&str]) -> Option<PathBuf> {
        let path = self.as_ref();
        let name = path.file_name().and_then(|value| value.to_str())?;
        let cut = match_suffixes(name, suffixes)?;
        Some(path.with_file_name(&name[..cut]))
    }
}

impl<T: AsRef<Path> + ?Sized> PathExt for T {}

// ---------------------------------------------------------------------------------------------- //
// Error Type

/// Describes path validation, component, and filesystem failures.
#[derive(thiserror::Error, Debug)]
pub enum PathError {
    /// The path has no existing target.
    #[error("path not found: {}", path.simplify_path().display())]
    NotFound { path: PathBuf },

    /// The path has an existing target when absence was required.
    #[error("path already exists: {}", path.simplify_path().display())]
    AlreadyExists { path: PathBuf },

    /// The target is not a regular file.
    #[error("not a file: {}", path.simplify_path().display())]
    NotAFile { path: PathBuf },

    /// The target is not a directory.
    #[error("not a directory: {}", path.simplify_path().display())]
    NotADir { path: PathBuf },

    /// The parent directory does not exist.
    #[error("cannot create '{}', parent directory '{}' does not exist", name, parent.simplify_path().display())]
    ParentNotFound { name: String, parent: PathBuf },

    /// A required lexical component is missing.
    #[error("missing {component}: {}", path.simplify_path().display())]
    MissingComponent {
        path: PathBuf,
        component: &'static str,
    },

    /// A filesystem operation failed.
    #[error("failed to {operation} '{}': {source}", path.simplify_path().display())]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// A native component cannot be represented as UTF-8.
    #[error("invalid UTF-8 in {component}: {}", path.simplify_path().display())]
    InvalidUtf8 {
        path: PathBuf,
        component: &'static str,
    },
}

// ---------------------------------------------------------------------------------------------- //
// Helpers

/// Validates a nonempty parent using one fallible metadata lookup.
fn check_parent_is_existing_dir(path: &Path) -> Result<(), PathError> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        let metadata = metadata_if_exists(parent)?.ok_or_else(|| PathError::ParentNotFound {
            name: display_name(path),
            parent: parent.to_path_buf(),
        })?;
        require_directory(parent, &metadata)?;
    }
    Ok(())
}

/// Returns a native component or a contextual missing-component error.
fn required_component<'a, T: ?Sized>(
    path: &Path,
    component: &'static str,
    value: Option<&'a T>,
) -> Result<&'a T, PathError> {
    value.ok_or_else(|| PathError::MissingComponent {
        path: path.to_path_buf(),
        component,
    })
}

/// Converts a required native component without confusing absence with invalid text.
fn os_str_to_str<'a>(
    path: &Path,
    component: &'static str,
    value: Option<&'a OsStr>,
) -> Result<&'a str, PathError> {
    required_component(path, component, value)?.to_str().ok_or_else(|| PathError::InvalidUtf8 {
        path: path.to_path_buf(),
        component,
    })
}

/// Checks target existence while preserving inspection errors.
fn try_exists(path: &Path) -> Result<bool, PathError> {
    path.try_exists().map_err(|source| PathError::Io {
        operation: "check existence",
        path: path.to_path_buf(),
        source,
    })
}

/// Queries following metadata and suppresses only a missing target.
fn metadata_if_exists(path: &Path) -> Result<Option<Metadata>, PathError> {
    fs::metadata(path).not_found_ok().map_err(|source| PathError::Io {
        operation: "inspect metadata",
        path: path.to_path_buf(),
        source,
    })
}

/// Returns following metadata or a contextual missing-target error.
fn required_metadata(path: &Path) -> Result<Metadata, PathError> {
    metadata_if_exists(path)?.ok_or_else(|| PathError::NotFound {
        path: path.to_path_buf(),
    })
}

/// Validates the directory type using already acquired metadata.
fn require_directory<'a>(path: &'a Path, metadata: &Metadata) -> Result<&'a Path, PathError> {
    if metadata.is_dir() {
        Ok(path)
    } else {
        Err(PathError::NotADir {
            path: path.to_path_buf(),
        })
    }
}

/// Creates a directory and validates an entry appearing during a creation race.
fn create_directory(path: &Path, recursive: bool) -> Result<&Path, PathError> {
    let result = if recursive {
        fs::create_dir_all(path)
    } else {
        fs::create_dir(path)
    };
    match result {
        Ok(()) => Ok(path),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            require_directory(path, &required_metadata(path)?)
        }
        Err(source) => Err(PathError::Io {
            operation: "create directory",
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Recreates a directory using standard removal and creation semantics.
fn recreate_directory(path: &Path, recursive: bool) -> Result<&Path, PathError> {
    if let Some(metadata) = metadata_if_exists(path)? {
        require_directory(path, &metadata)?;
        fs::remove_dir_all(path).map_err(|source| PathError::Io {
            operation: "remove directory",
            path: path.to_path_buf(),
            source,
        })?;
    }
    create_directory(path, recursive)
}

/// Returns a filename for a parent-missing diagnostic.
fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.simplify_path().display().to_string())
}

/// Matches suffixes while rejecting results that are not ordinary filename bases.
fn match_suffixes(name: &str, suffixes: &[&str]) -> Option<usize> {
    if suffixes.is_empty() || name.is_empty() || name.ends_with('.') {
        return None;
    }
    let mut end = name.len();
    for expected in suffixes.iter().rev() {
        let before = &name[..end];
        let dot = before.rfind('.')?;
        let segment = &before[dot + 1..];
        if segment.is_empty() || !segment.eq_ignore_ascii_case(expected) {
            return None;
        }
        end = dot;
    }
    if matches!(&name[..end], "" | "." | "..") {
        None
    } else {
        Some(end)
    }
}

// ---------------------------------------------------------------------------------------------- //

#[cfg(test)]
mod tests;
