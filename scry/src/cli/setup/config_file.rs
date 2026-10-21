//! Parses an explicit file and optional key path from one native CLI argument.

use crate::key_path::KeyPathError;
use crate::KeyPath;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::str::FromStr;

// ---------------------------------------------------------------------------------------------- //

/// Identifies an explicit config file and an optional subtree within its returned value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFile {
    /// Native filesystem path, with escaped file delimiters decoded.
    pub path: PathBuf,
    /// Selected key path, or the empty path for the entire file value.
    pub selection: KeyPath,
}

impl ConfigFile {
    /// Parses `FILE` or `FILE::KEYPATH` without consulting the filesystem.
    ///
    /// The first unescaped `::` separates the file from its selector. In the file portion,
    /// `\:` represents one literal colon. In a run of backslashes immediately before a colon,
    /// pairs represent literal backslashes and an unpaired backslash escapes the colon. All other
    /// backslashes remain unchanged. The selector uses ordinary [`KeyPath`] syntax and must be
    /// nonempty Unicode text. File paths retain their native encoding, including filenames that
    /// cannot be represented as Unicode.
    ///
    /// For example, `archive.rhai::prune` selects `prune`, while `file\::part.rhai` names the
    /// literal file `file::part.rhai`. A single colon, as in `release:preview.rhai`, needs no
    /// escaping. Ordinary Windows paths such as `C:\configs\archive.rhai::prune` retain their
    /// separators. Escaping applies only to the filename. A selector such as
    /// `["key::with:colons"]` is passed unchanged to [`KeyPath`].
    pub fn parse(argument: &OsStr) -> Result<Self, ConfigFileError> {
        let bytes = argument.as_encoded_bytes();
        let mut file = Vec::with_capacity(bytes.len());
        let mut offset = 0;
        let mut selector = None;
        while offset < bytes.len() {
            if bytes[offset] == b'\\' {
                let start = offset;
                while offset < bytes.len() && bytes[offset] == b'\\' {
                    offset += 1;
                }
                if offset == bytes.len() || bytes[offset] != b':' {
                    file.extend_from_slice(&bytes[start..offset]);
                    continue;
                }
                let count = offset - start;
                file.extend(std::iter::repeat_n(b'\\', count / 2));
                if count % 2 == 1 {
                    file.push(b':');
                    offset += 1;
                    continue;
                }
            }
            if bytes[offset..].starts_with(b"::") {
                selector = Some(&bytes[offset + 2..]);
                break;
            } else {
                file.push(bytes[offset]);
                offset += 1;
            }
        }
        if file.is_empty() {
            return Err(ConfigFileError::EmptyFile);
        }
        let selection = match selector {
            None => KeyPath::new(),
            Some([]) => return Err(ConfigFileError::EmptySelection),
            Some(selector) => std::str::from_utf8(selector)
                .map_err(|_| ConfigFileError::InvalidSelectionEncoding)?
                .parse()
                .map_err(|source| ConfigFileError::InvalidSelection { source })?,
        };

        // SAFETY: The bytes originate from this platform's OsStr encoding. The parser only
        // removes complete ASCII delimiters or decodes ASCII escapes immediately before a
        // retained colon. Non-ASCII sequences are unchanged, and edits occur at ASCII boundaries.
        let path = PathBuf::from(unsafe { OsStr::from_encoded_bytes_unchecked(&file) });
        Ok(Self { path, selection })
    }
}

impl FromStr for ConfigFile {
    type Err = ConfigFileError;

    fn from_str(argument: &str) -> Result<Self, Self::Err> {
        Self::parse(OsStr::new(argument))
    }
}

/// Describes invalid explicit config file or selector syntax.
#[derive(Debug, thiserror::Error)]
pub enum ConfigFileError {
    /// The argument has no filesystem path before its selector.
    #[error("config file path must not be empty")]
    EmptyFile,
    /// A separator is present without a following selector.
    #[error("config selector after '::' must not be empty")]
    EmptySelection,
    /// The selector contains native characters that are not valid Unicode.
    #[error("config selector must be valid Unicode")]
    InvalidSelectionEncoding,
    /// The selector does not follow key-path syntax.
    #[error("invalid config selector: {source}")]
    InvalidSelection {
        #[source]
        source: KeyPathError,
    },
}

// ---------------------------------------------------------------------------------------------- //

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn ordinary_files_and_single_colons_remain_whole_payload_paths() {
        for filename in [
            "config.rhai",
            "missing-file.json",
            "report:alternate.rhai",
            "配置.rhai",
        ] {
            let parsed = ConfigFile::parse(OsStr::new(filename)).unwrap();
            assert_eq!(parsed.path, PathBuf::from(filename));
            assert!(parsed.selection.is_empty());
        }
    }

    #[test]
    fn windows_drive_unc_and_extended_paths_preserve_their_backslashes() {
        for filename in [
            r"U:\configs\enhance-upscale.rhai",
            r"U:enhance-upscale.rhai",
            r"\\server\share\enhance-upscale.rhai",
            r"\\?\U:\configs\enhance-upscale.rhai",
            r"\\?\UNC\server\share\enhance-upscale.rhai",
            r"\\.\U:\configs\enhance-upscale.rhai",
        ] {
            let parsed: ConfigFile = format!("{filename}::mask.split").parse().unwrap();
            assert_eq!(parsed.path, PathBuf::from(filename));
            assert_eq!(parsed.selection, KeyPath::from_keys(["mask", "split"]));
        }
    }

    #[test]
    fn selectors_keep_quoted_colons_unicode_and_array_indices() {
        for selector in [
            "mask.split",
            r#"["segment-setup"]"#,
            r#"["$self"]"#,
            r#"["key:with::colons"]"#,
            r#"variants[2]["点:鍵::名"]"#,
            r#"["quote\"and\\slash::key"]"#,
        ] {
            let parsed: ConfigFile = format!("config.rhai::{selector}").parse().unwrap();
            assert_eq!(parsed.path, PathBuf::from("config.rhai"));
            assert_eq!(parsed.selection, selector.parse::<KeyPath>().unwrap());
        }
    }

    #[test]
    fn escaped_file_delimiters_are_decoded_without_touching_other_backslashes() {
        for (argument, filename, selector) in [
            (r"name\::alternate.rhai", "name::alternate.rhai", None),
            (r"name\::alternate.rhai::mask.split", "name::alternate.rhai", Some("mask.split")),
            (r"dir\name\\\::alternate.rhai::child", r"dir\name\::alternate.rhai", Some("child")),
            (r"one\::two\::three.rhai", "one::two::three.rhai", None),
            (r"\::", "::", None),
            (r"file\\::child", r"file\", Some("child")),
            (r"file\:::child", "file:", Some("child")),
            (r"file\\\:name.rhai::child", r"file\:name.rhai", Some("child")),
            (r"file\:name.rhai", "file:name.rhai", None),
        ] {
            let parsed: ConfigFile = argument.parse().unwrap();
            assert_eq!(parsed.path, PathBuf::from(filename));
            assert_eq!(parsed.selection, selector.map(|s| s.parse().unwrap()).unwrap_or_default());
        }
    }

    #[test]
    fn empty_files_and_empty_or_malformed_selectors_are_rejected() {
        for argument in ["", "::", "::mask.split"] {
            assert!(matches!(argument.parse::<ConfigFile>(), Err(ConfigFileError::EmptyFile)));
        }
        assert!(matches!(
            "config.rhai::".parse::<ConfigFile>(),
            Err(ConfigFileError::EmptySelection)
        ));
        for selector in ["mask.", "foo-bar", "mask::split", r#"["unterminated]"#] {
            assert!(matches!(
                format!("config.rhai::{selector}").parse::<ConfigFile>(),
                Err(ConfigFileError::InvalidSelection { .. })
            ));
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn native_non_unicode_file_names_survive_with_and_without_selectors() {
        let filename = non_unicode();
        let parsed = ConfigFile::parse(&filename).unwrap();
        assert_eq!(parsed.path.as_os_str(), filename);
        assert!(parsed.selection.is_empty());

        let mut argument = filename.clone();
        argument.push("::mask.split");
        let parsed = ConfigFile::parse(&argument).unwrap();
        assert_eq!(parsed.path.as_os_str(), filename);
        assert_eq!(parsed.selection, KeyPath::from_keys(["mask", "split"]));

        let mut argument = filename.clone();
        argument.push(r"\::alternate.rhai::child");
        let mut expected = filename;
        expected.push("::alternate.rhai");
        let parsed = ConfigFile::parse(&argument).unwrap();
        assert_eq!(parsed.path.as_os_str(), expected);
        assert_eq!(parsed.selection, KeyPath::from_keys(["child"]));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn selectors_reject_non_unicode_native_characters() {
        let mut argument = OsString::from("config.rhai::");
        argument.push(non_unicode());
        assert!(matches!(
            ConfigFile::parse(&argument),
            Err(ConfigFileError::InvalidSelectionEncoding)
        ));
    }

    #[cfg(unix)]
    fn non_unicode() -> OsString {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![b'f', 0xff, b'o'])
    }

    #[cfg(windows)]
    fn non_unicode() -> OsString {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[u16::from(b'f'), 0xd800, u16::from(b'o')])
    }
}
