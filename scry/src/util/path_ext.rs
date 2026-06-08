//! Utilities for path handling with more helpful error messages.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------------- //

/// Extension trait for path validation and directory operations.
///
/// Provides chainable methods that return `Result<&Path, PathError>`, allowing validation
/// and directory creation to be composed fluently while preserving the original path.
pub trait PathExt: AsRef<Path> {
    // ------------------------------------------------------------------------------------------ //
    // Existence Checks

    /// Validates that the path exists on the filesystem.
    fn require_exists(&self) -> Result<&Path, PathError> {
        let p = self.as_ref();
        if try_exists(p)? {
            Ok(p)
        } else {
            Err(PathError::NotFound {
                path: p.to_path_buf(),
            })
        }
    }

    /// Validates that the path does not exist on the filesystem.
    fn require_missing(&self) -> Result<&Path, PathError> {
        let p = self.as_ref();
        if try_exists(p)? {
            Err(PathError::AlreadyExists {
                path: p.to_path_buf(),
            })
        } else {
            Ok(p)
        }
    }

    /// Validates that the path exists and is a file.
    fn require_file(&self) -> Result<&Path, PathError> {
        let p = self.require_exists()?;
        if p.is_file() {
            Ok(p)
        } else {
            Err(PathError::NotAFile {
                path: p.to_path_buf(),
            })
        }
    }

    /// Validates that the path exists and is a directory.
    fn require_dir(&self) -> Result<&Path, PathError> {
        let p = self.require_exists()?;
        if p.is_dir() {
            Ok(p)
        } else {
            Err(PathError::NotADir {
                path: p.to_path_buf(),
            })
        }
    }

    /// Validates that the parent directory exists and is a directory.
    ///
    /// If the path has no parent component (e.g. "file.txt" in CWD), this is a no-op.
    fn require_parent_dir(&self) -> Result<&Path, PathError> {
        let p = self.as_ref();
        check_parent_is_existing_dir(p)?;
        Ok(p)
    }

    // ------------------------------------------------------------------------------------------ //
    // Directory Creation

    /// Ensures the directory exists, creating it if needed.
    ///
    /// The parent directory must already exist.
    fn ensure_dir(&self) -> Result<&Path, PathError> {
        let p = self.as_ref();
        // Check parent first (before checking target) to give clear error if parent is a file
        check_parent_is_existing_dir(p)?;

        if try_exists(p)? {
            return if p.is_dir() {
                Ok(p)
            } else {
                Err(PathError::NotADir {
                    path: p.to_path_buf(),
                })
            };
        }
        match std::fs::create_dir(p) {
            Ok(()) => Ok(p),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Race: something appeared between our exists check and mkdir
                if p.is_dir() {
                    Ok(p)
                } else {
                    Err(PathError::NotADir {
                        path: p.to_path_buf(),
                    })
                }
            }
            Err(e) => Err(PathError::Io {
                path: p.to_path_buf(),
                source: e,
            }),
        }
    }

    /// Ensures the directory and all parents exist.
    fn ensure_dir_all(&self) -> Result<&Path, PathError> {
        let p = self.as_ref();
        if try_exists(p)? {
            return if p.is_dir() {
                Ok(p)
            } else {
                Err(PathError::NotADir {
                    path: p.to_path_buf(),
                })
            };
        }
        match std::fs::create_dir_all(p) {
            Ok(()) => Ok(p),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Race: something appeared between our exists check and mkdir
                if p.is_dir() {
                    Ok(p)
                } else {
                    Err(PathError::NotADir {
                        path: p.to_path_buf(),
                    })
                }
            }
            Err(e) => Err(PathError::Io {
                path: p.to_path_buf(),
                source: e,
            }),
        }
    }

    /// Deletes the directory if it exists, then creates it fresh.
    ///
    /// The parent directory must already exist.
    fn recreate_dir(&self) -> Result<&Path, PathError> {
        let p = self.as_ref();
        // Check parent first (before checking target) to give clear error if parent is a file
        check_parent_is_existing_dir(p)?;

        if try_exists(p)? {
            if !p.is_dir() {
                return Err(PathError::NotADir {
                    path: p.to_path_buf(),
                });
            }
            std::fs::remove_dir_all(p).map_err(|e| PathError::Io {
                path: p.to_path_buf(),
                source: e,
            })?;
        }
        match std::fs::create_dir(p) {
            Ok(()) => Ok(p),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Race: something appeared between our remove and mkdir
                if p.is_dir() {
                    Ok(p)
                } else {
                    Err(PathError::NotADir {
                        path: p.to_path_buf(),
                    })
                }
            }
            Err(e) => Err(PathError::Io {
                path: p.to_path_buf(),
                source: e,
            }),
        }
    }

    /// Deletes the directory if it exists, then creates it and all parents.
    fn recreate_dir_all(&self) -> Result<&Path, PathError> {
        let p = self.as_ref();
        if try_exists(p)? {
            if !p.is_dir() {
                return Err(PathError::NotADir {
                    path: p.to_path_buf(),
                });
            }
            std::fs::remove_dir_all(p).map_err(|e| PathError::Io {
                path: p.to_path_buf(),
                source: e,
            })?;
        }
        match std::fs::create_dir_all(p) {
            Ok(()) => Ok(p),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Race: something appeared between our remove and mkdir
                if p.is_dir() {
                    Ok(p)
                } else {
                    Err(PathError::NotADir {
                        path: p.to_path_buf(),
                    })
                }
            }
            Err(e) => Err(PathError::Io {
                path: p.to_path_buf(),
                source: e,
            }),
        }
    }

    /// Ensures the parent directory exists, creating it and all ancestors if needed.
    ///
    /// Useful before writing a file: `path.ensure_parent_dir()?; fs::write(&path, data)?;`
    fn ensure_parent_dir(&self) -> Result<&Path, PathError> {
        let p = self.as_ref();
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                parent.ensure_dir_all()?;
            }
        }
        Ok(p)
    }

    // ------------------------------------------------------------------------------------------ //
    // String Conversions

    /// Returns the full path as a string slice.
    fn path_str(&self) -> Result<&str, PathError> {
        let p = self.as_ref();
        p.to_str().ok_or_else(|| PathError::InvalidUtf8 {
            path: p.to_path_buf(),
            component: "path",
        })
    }

    /// Returns the file name as a string slice.
    fn file_name_str(&self) -> Result<&str, PathError> {
        let p = self.as_ref();
        os_str_to_str(p, "file name", p.file_name())
    }

    /// Returns the file stem as a string slice.
    fn stem_str(&self) -> Result<&str, PathError> {
        let p = self.as_ref();
        os_str_to_str(p, "file stem", p.file_stem())
    }

    /// Returns the file extension as a string slice.
    fn ext_str(&self) -> Result<&str, PathError> {
        let p = self.as_ref();
        os_str_to_str(p, "file extension", p.extension())
    }

    /// Checks whether the file extension matches the given extension (case-insensitive).
    fn has_ext(&self, ext: &str) -> bool {
        self.as_ref()
            .extension()
            .and_then(|s| s.to_str())
            .map(|e| e.eq_ignore_ascii_case(ext))
            .unwrap_or(false)
    }

    /// Checks whether the file extension matches any of the given extensions (case-insensitive).
    fn has_any_ext(&self, exts: &[&str]) -> bool {
        let Some(ext) = self.as_ref().extension().and_then(|s| s.to_str()) else {
            return false;
        };
        exts.iter().any(|e| ext.eq_ignore_ascii_case(e))
    }

    /// Checks whether the file name ends with the given suffixes (case-insensitive).
    ///
    /// Suffixes are ordered left-to-right, e.g., `["tar", "gz"]` matches `.tar.gz`.
    /// Returns `false` if the file name is missing, non-UTF8, or doesn't match.
    ///
    /// ```
    /// # use std::path::Path;
    /// # use scry::util::PathExt;
    /// assert!(Path::new("archive.tar.gz").ends_with_suffixes(&["tar", "gz"]));
    /// assert!(Path::new("archive.tar.gz").ends_with_suffixes(&["gz"]));
    /// assert!(!Path::new("archive.tar.gz").ends_with_suffixes(&["zip"]));
    /// ```
    fn ends_with_suffixes(&self, suffixes: &[&str]) -> bool {
        let Some(name) = self.as_ref().file_name().and_then(|s| s.to_str()) else {
            return false;
        };
        match_suffixes(name, suffixes).is_some()
    }

    /// Strips the given extension suffixes from the file name, returning a new path.
    ///
    /// Suffixes are ordered left-to-right, e.g., `["tar", "gz"]` strips `.tar.gz`.
    /// Returns `None` if the file name is missing, non-UTF8, doesn't match, or stripping
    /// would leave an empty base name.
    ///
    /// ```
    /// # use std::path::Path;
    /// # use scry::util::PathExt;
    /// assert_eq!(
    ///     Path::new("archive.tar.gz").strip_suffixes(&["tar", "gz"]),
    ///     Some("archive".into())
    /// );
    /// assert_eq!(
    ///     Path::new("my.archive.tar.gz").strip_suffixes(&["gz"]),
    ///     Some("my.archive.tar".into())
    /// );
    /// ```
    fn strip_suffixes(&self, suffixes: &[&str]) -> Option<PathBuf> {
        let p = self.as_ref();
        let name = p.file_name().and_then(|s| s.to_str())?;
        let cut = match_suffixes(name, suffixes)?;
        let base = &name[..cut];
        if base.is_empty() {
            return None;
        }
        Some(p.with_file_name(base))
    }
}

impl<T: AsRef<Path>> PathExt for T {}

// ---------------------------------------------------------------------------------------------- //
// Error Type

/// Errors that occur during path validation and directory operations.
#[derive(thiserror::Error, Debug)]
pub enum PathError {
    /// Path does not exist.
    #[error("path not found: {}", path.display())]
    NotFound { path: PathBuf },

    /// Path already exists when it should not.
    #[error("path already exists: {}", path.display())]
    AlreadyExists { path: PathBuf },

    /// Expected a file but found something else.
    #[error("not a file: {}", path.display())]
    NotAFile { path: PathBuf },

    /// Expected a directory but found something else.
    #[error("not a directory: {}", path.display())]
    NotADir { path: PathBuf },

    /// Parent directory does not exist.
    #[error("cannot create '{}', parent directory '{}' does not exist", name, parent.display())]
    ParentNotFound { name: String, parent: PathBuf },

    /// Path is missing a required component (e.g., no file extension).
    #[error("missing {component}: {}", path.display())]
    MissingComponent {
        path: PathBuf,
        component: &'static str,
    },

    /// I/O operation failed.
    #[error("I/O error on '{}': {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Path contains invalid UTF-8.
    #[error("invalid UTF-8 in {component}: {}", path.display())]
    InvalidUtf8 {
        path: PathBuf,
        component: &'static str,
    },
}

// ---------------------------------------------------------------------------------------------- //
// Helper Functions

/// Validates that the parent directory exists and is a directory.
///
/// This is a no-op for paths whose parent is empty (e.g. "file.txt" in the CWD).
fn check_parent_is_existing_dir(p: &Path) -> Result<(), PathError> {
    if let Some(parent) = p.parent() {
        // For paths like "file.txt", parent() is Some("") on many platforms.
        if !parent.as_os_str().is_empty() {
            if !try_exists(parent)? {
                return Err(PathError::ParentNotFound {
                    name: display_name(p),
                    parent: parent.to_path_buf(),
                });
            }
            if !parent.is_dir() {
                return Err(PathError::NotADir {
                    path: parent.to_path_buf(),
                });
            }
        }
    }
    Ok(())
}

/// Converts an optional OsStr to a string, distinguishing missing vs invalid UTF-8.
fn os_str_to_str<'a>(
    path: &Path,
    component: &'static str,
    value: Option<&'a OsStr>,
) -> Result<&'a str, PathError> {
    let value = value.ok_or_else(|| PathError::MissingComponent {
        path: path.to_path_buf(),
        component,
    })?;
    value.to_str().ok_or_else(|| PathError::InvalidUtf8 {
        path: path.to_path_buf(),
        component,
    })
}

/// Checks if a path exists, properly reporting I/O errors instead of returning false.
fn try_exists(path: &Path) -> Result<bool, PathError> {
    path.try_exists().map_err(|e| PathError::Io {
        path: path.to_path_buf(),
        source: e,
    })
}

/// Returns a display name for a path, for use in error messages.
fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Matches extension suffixes against a file name, returning the cut index if matched.
///
/// Returns `Some(cut)` where `cut` is the byte index where the base name ends (before the
/// matched suffixes). Returns `None` if no match or if matching would leave an empty or
/// invalid base (empty string or just `.`).
fn match_suffixes(name: &str, suffixes: &[&str]) -> Option<usize> {
    if suffixes.is_empty() || name.is_empty() || name.ends_with('.') {
        return None;
    }

    let mut end = name.len();
    for expected in suffixes.iter().rev() {
        let before = &name[..end];
        let dot = before.rfind('.')?;
        let seg = &before[dot + 1..];
        if seg.is_empty() || !seg.eq_ignore_ascii_case(expected) {
            return None;
        }
        end = dot;
    }

    // Reject if matching would leave an empty or invalid base
    let base = &name[..end];
    if base.is_empty() || base == "." {
        return None;
    }

    Some(end)
}

// ---------------------------------------------------------------------------------------------- //
// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn unique_temp_dir(tag: &str) -> PathBuf {
        let pid = std::process::id();
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("scry_path_ext_test_{tag}_{pid}_{nanos}"))
    }

    #[test]
    fn ext_str_missing_gives_missing_component_not_invalid_utf8() {
        let p = Path::new("no_extension");
        match p.ext_str() {
            Err(PathError::MissingComponent { component, .. }) => {
                assert_eq!(component, "file extension");
            }
            other => panic!("expected MissingComponent, got: {other:?}"),
        }
    }

    #[test]
    fn file_name_str_missing_gives_missing_component() {
        let p = Path::new("");
        match p.file_name_str() {
            Err(PathError::MissingComponent { component, .. }) => {
                assert_eq!(component, "file name");
            }
            other => panic!("expected MissingComponent, got: {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn ext_str_invalid_utf8_on_unix() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let mut bytes = b"foo.".to_vec();
        bytes.push(0xFF);
        let os = OsString::from_vec(bytes);
        let p = PathBuf::from(os);

        match p.ext_str() {
            Err(PathError::InvalidUtf8 { component, .. }) => {
                assert_eq!(component, "file extension");
            }
            other => panic!("expected InvalidUtf8, got: {other:?}"),
        }
    }

    #[test]
    fn ensure_dir_errors_if_path_is_file() {
        let base = unique_temp_dir("ensure_dir_file");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();

        let file_path = base.join("not_a_dir");
        fs::write(&file_path, b"hi").unwrap();

        let res = file_path.ensure_dir();
        assert!(matches!(res, Err(PathError::NotADir { .. })));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn ensure_dir_all_errors_if_path_is_file() {
        let base = unique_temp_dir("ensure_dir_all_file");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();

        let file_path = base.join("not_a_dir");
        fs::write(&file_path, b"hi").unwrap();

        let res = file_path.ensure_dir_all();
        assert!(matches!(res, Err(PathError::NotADir { .. })));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn ensure_dir_errors_if_parent_missing() {
        let base = unique_temp_dir("ensure_dir_parent_missing");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();

        let p = base.join("missing_parent").join("child");
        let res = p.ensure_dir();

        match res {
            Err(PathError::ParentNotFound { parent, .. }) => {
                assert_eq!(parent, base.join("missing_parent"));
            }
            other => panic!("expected ParentNotFound, got: {other:?}"),
        }

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn ensure_dir_all_creates_parents() {
        let base = unique_temp_dir("ensure_dir_all_parents");
        let _ = fs::remove_dir_all(&base);

        let p = base.join("a").join("b").join("c");
        p.ensure_dir_all().unwrap();

        assert!(p.is_dir());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn recreate_dir_errors_if_path_is_file() {
        let base = unique_temp_dir("recreate_dir_file");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();

        let file_path = base.join("not_a_dir");
        fs::write(&file_path, b"hi").unwrap();

        let res = file_path.recreate_dir();
        assert!(matches!(res, Err(PathError::NotADir { .. })));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn recreate_dir_all_removes_existing_contents() {
        let base = unique_temp_dir("recreate_dir_all_removes");
        let _ = fs::remove_dir_all(&base);

        let dir = base.join("x").join("y");
        dir.ensure_dir_all().unwrap();

        let file = dir.join("old.txt");
        let mut f = fs::File::create(&file).unwrap();
        writeln!(f, "hello").unwrap();
        assert!(file.exists());

        dir.recreate_dir_all().unwrap();
        assert!(dir.is_dir());
        assert!(!file.exists());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn has_ext_is_case_insensitive() {
        let p = Path::new("config.RHAI");
        assert!(p.has_ext("rhai"));
        assert!(p.has_ext("RHAI"));
        assert!(!p.has_ext("toml"));
    }

    #[test]
    fn ensure_dir_errors_if_parent_is_file() {
        let base = unique_temp_dir("ensure_dir_parent_is_file");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();

        // Create a file where the "parent" should be
        let fake_parent = base.join("not_a_dir");
        fs::write(&fake_parent, b"hi").unwrap();

        // Try to create a child directory under the file
        let child = fake_parent.join("child");
        let res = child.ensure_dir();

        match res {
            Err(PathError::NotADir { path }) => {
                assert_eq!(path, fake_parent);
            }
            other => panic!("expected NotADir for parent, got: {other:?}"),
        }

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn recreate_dir_errors_if_parent_is_file() {
        let base = unique_temp_dir("recreate_dir_parent_is_file");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();

        // Create a file where the "parent" should be
        let fake_parent = base.join("not_a_dir");
        fs::write(&fake_parent, b"hi").unwrap();

        // Try to recreate a child directory under the file
        let child = fake_parent.join("child");
        let res = child.recreate_dir();

        match res {
            Err(PathError::NotADir { path }) => {
                assert_eq!(path, fake_parent);
            }
            other => panic!("expected NotADir for parent, got: {other:?}"),
        }

        let _ = fs::remove_dir_all(&base);
    }

    // --- require_parent_dir() tests ---

    #[test]
    fn require_parent_dir_ok_for_cwd_relative_file() {
        // No filesystem assumptions needed: parent is "" => no-op
        let p = Path::new("file.txt");
        p.require_parent_dir().unwrap();
    }

    #[test]
    fn require_parent_dir_errors_if_parent_missing() {
        let base = unique_temp_dir("require_parent_dir_parent_missing");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();

        let p = base.join("missing_parent").join("child.txt");
        let res = p.require_parent_dir();

        match res {
            Err(PathError::ParentNotFound { parent, .. }) => {
                assert_eq!(parent, base.join("missing_parent"));
            }
            other => panic!("expected ParentNotFound, got: {other:?}"),
        }

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn require_parent_dir_errors_if_parent_is_file() {
        let base = unique_temp_dir("require_parent_dir_parent_is_file");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();

        let fake_parent = base.join("not_a_dir");
        fs::write(&fake_parent, b"hi").unwrap();

        let child = fake_parent.join("child.txt");
        let res = child.require_parent_dir();

        match res {
            Err(PathError::NotADir { path }) => {
                assert_eq!(path, fake_parent);
            }
            other => panic!("expected NotADir for parent, got: {other:?}"),
        }

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn ends_with_suffixes_matches_compound() {
        let p = Path::new("archive.tar.gz");
        assert!(p.ends_with_suffixes(&["tar", "gz"]));
        assert!(p.ends_with_suffixes(&["gz"]));
        assert!(!p.ends_with_suffixes(&["zip"]));
        assert!(!p.ends_with_suffixes(&["tar", "zip"]));
    }

    #[test]
    fn ends_with_suffixes_case_insensitive() {
        let p = Path::new("ARCHIVE.TAR.GZ");
        assert!(p.ends_with_suffixes(&["tar", "gz"]));
        assert!(p.ends_with_suffixes(&["TAR", "GZ"]));
    }

    #[test]
    fn ends_with_suffixes_empty_returns_false() {
        let p = Path::new("archive.tar.gz");
        assert!(!p.ends_with_suffixes(&[]));
    }

    #[test]
    fn ends_with_suffixes_dotfile_not_matched_as_suffix() {
        // ".bashrc" should NOT match ["bashrc"] because that would consume the leading dot
        let p = Path::new(".bashrc");
        assert!(!p.ends_with_suffixes(&["bashrc"]));
    }

    #[test]
    fn ends_with_suffixes_dotfile_with_extension() {
        // ".config.json" should match ["json"]
        let p = Path::new(".config.json");
        assert!(p.ends_with_suffixes(&["json"]));
        // But ["config", "json"] would leave just "." as base, so no match
        assert!(!p.ends_with_suffixes(&["config", "json"]));
    }

    #[test]
    fn strip_suffixes_compound() {
        let p = Path::new("my.archive.tar.gz");
        assert_eq!(p.strip_suffixes(&["tar", "gz"]), Some(PathBuf::from("my.archive")));
        assert_eq!(p.strip_suffixes(&["gz"]), Some(PathBuf::from("my.archive.tar")));
        assert_eq!(p.strip_suffixes(&["zip"]), None);
    }

    #[test]
    fn strip_suffixes_preserves_parent_path() {
        let p = Path::new("/some/path/archive.tar.gz");
        assert_eq!(p.strip_suffixes(&["tar", "gz"]), Some(PathBuf::from("/some/path/archive")));
    }

    #[test]
    fn strip_suffixes_dotfile() {
        // ".config.json" stripping ["json"] yields ".config"
        let p = Path::new(".config.json");
        assert_eq!(p.strip_suffixes(&["json"]), Some(PathBuf::from(".config")));
    }

    #[test]
    fn strip_suffixes_empty_base_returns_none() {
        // Stripping all of ".tar.gz" from ".tar.gz" would leave empty base
        let p = Path::new(".tar.gz");
        assert_eq!(p.strip_suffixes(&["tar", "gz"]), None);
    }

    #[test]
    fn strip_suffixes_dot_base_returns_none() {
        // "..json" stripping ["json"] would leave just "." which is invalid
        let p = Path::new("..json");
        assert!(!p.ends_with_suffixes(&["json"]));
        assert_eq!(p.strip_suffixes(&["json"]), None);
    }
}
