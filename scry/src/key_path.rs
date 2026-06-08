//! Path navigation for configuration trees.
//!
//! A [`KeyPath`] represents a location in a nested configuration structure,
//! supporting both map keys and array indices. Paths can be constructed
//! programmatically or parsed from strings using a simple dotted syntax.
//!
//! # Path Syntax
//!
//! - **Dotted keys**: `server.port` navigates to `server`, then `port`
//! - **Array indices**: `items[0]` navigates to the first element of `items`
//! - **Mixed paths**: `servers[0].port` combines both
//! - **Quoted keys**: `["dotted.key"]` for keys containing special characters
//!
//! # Examples
//!
//! ```
//! use scry::KeyPath;
//!
//! // Parse from string
//! let path: KeyPath = "server.port".parse().unwrap();
//!
//! // Build programmatically
//! let path = KeyPath::new().push_key("server").push_key("port");
//!
//! // From an array of keys
//! let path = KeyPath::from_keys(["server", "port"]);
//! ```

use std::fmt::{self, Write};

use crate::BoxedError;

// ---------------------------------------------------------------------------------------------- //
// KeyPath

/// A path to a location in a configuration tree.
///
/// Paths consist of a sequence of segments, where each segment is either
/// a string key (for maps) or a numeric index (for arrays).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct KeyPath {
    pub(crate) segs: Vec<Segment>,
}

impl KeyPath {
    /// Creates an empty path (refers to the root).
    pub fn new() -> Self {
        Self { segs: Vec::new() }
    }

    /// Creates a path from a single array index.
    pub fn from_index(idx: usize) -> Self {
        Self {
            segs: vec![Segment::Index(idx)],
        }
    }

    /// Creates a path from an iterator of string keys.
    pub fn from_keys(keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            segs: keys.into_iter().map(|k| Segment::Key(k.into())).collect(),
        }
    }

    /// Returns a new path with a key segment appended.
    pub fn push_key(&self, key: impl Into<String>) -> Self {
        let mut new_path = self.clone();
        new_path.segs.push(Segment::Key(key.into()));
        new_path
    }

    /// Returns a new path with an index segment appended.
    pub fn push_index(&self, idx: usize) -> Self {
        let mut new_path = self.clone();
        new_path.segs.push(Segment::Index(idx));
        new_path
    }

    /// Returns true if this path is empty (refers to the root).
    pub fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }

    /// Returns an iterator over the segments in this path.
    pub fn iter(&self) -> impl Iterator<Item = &Segment> {
        self.segs.iter()
    }

    /// Returns a new path by appending all segments of `other` to this path.
    pub fn join(&self, other: &KeyPath) -> Self {
        let mut new_path = self.clone();
        new_path.segs.extend_from_slice(&other.segs);
        new_path
    }
}

impl fmt::Display for KeyPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut iter = self.segs.iter().peekable();
        while let Some(seg) = iter.next() {
            write!(f, "{}", seg)?;
            // Add dot before next segment if it's an identifier key
            if let Some(Segment::Key(key)) = iter.peek() {
                if is_identifier(key) {
                    write!(f, ".")?;
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------- //

/// A single segment in a key path: either a map key or an array index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Segment {
    Key(String),
    Index(usize),
}

impl fmt::Display for Segment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Segment::Key(key) => {
                if is_identifier(key) {
                    write!(f, "{}", key)
                } else {
                    write!(f, "[{}]", quote_string(key))
                }
            }
            Segment::Index(idx) => write!(f, "[{}]", idx),
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// KeyPathError

/// Error returned when parsing a key path string fails.
#[derive(thiserror::Error, Debug)]
#[error("{message}")]
pub struct KeyPathError {
    message: String,
    #[source]
    source: Option<BoxedError>,
}

impl KeyPathError {
    /// Creates a new error with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Parsing

impl std::str::FromStr for KeyPath {
    type Err = KeyPathError;

    fn from_str(mut s: &str) -> std::result::Result<Self, KeyPathError> {
        if s.is_empty() {
            return Err(KeyPathError::new("empty path"));
        }
        if s.starts_with('.') {
            return Err(KeyPathError::new("path cannot start with '.'"));
        }

        let mut segs = Vec::new();

        while !s.is_empty() {
            match s.chars().next().unwrap() {
                '[' => {
                    s = &s[1..]; // skip '['

                    if s.starts_with('"') {
                        // Quoted key: ["some key"]
                        s = &s[1..]; // skip opening quote
                        let mut key = String::new();

                        loop {
                            let c = s.chars().next().ok_or_else(|| {
                                KeyPathError::new("unterminated string in bracket notation")
                            })?;
                            s = &s[c.len_utf8()..];

                            match c {
                                '"' => break,
                                '\\' => {
                                    let esc = s.chars().next().ok_or_else(|| {
                                        KeyPathError::new("unterminated escape sequence")
                                    })?;
                                    s = &s[esc.len_utf8()..];
                                    match esc {
                                        '"' => key.push('"'),
                                        '\\' => key.push('\\'),
                                        'n' => key.push('\n'),
                                        'r' => key.push('\r'),
                                        't' => key.push('\t'),
                                        other => {
                                            return Err(KeyPathError::new(format!(
                                                "unknown escape sequence: \\{other}"
                                            )))
                                        }
                                    }
                                }
                                other => key.push(other),
                            }
                        }

                        if !s.starts_with(']') {
                            return Err(KeyPathError::new("expected ']' after quoted key"));
                        }
                        s = &s[1..];
                        segs.push(Segment::Key(key));
                    } else {
                        // Numeric index: [123]
                        let end = s
                            .find(']')
                            .ok_or_else(|| KeyPathError::new("unterminated index bracket"))?;
                        let idx: usize = s[..end]
                            .parse()
                            .map_err(|_| KeyPathError::new("invalid array index"))?;
                        s = &s[end + 1..];
                        segs.push(Segment::Index(idx));
                    }
                }
                '.' => {
                    return Err(KeyPathError::new("empty segment (consecutive dots)"));
                }
                _ => {
                    // Identifier: read until dot or bracket
                    let end = s.find(['.', '[']).unwrap_or(s.len());
                    let seg = &s[..end];
                    if !is_identifier(seg) {
                        return Err(KeyPathError::new(format!(
                            "invalid identifier '{seg}'; use [\"...\"] quoting for special characters"
                        )));
                    }
                    segs.push(Segment::Key(seg.to_string()));
                    s = &s[end..];
                }
            }

            // Handle separators after parsing a segment
            if s.starts_with('.') {
                s = &s[1..]; // consume the dot
                if s.is_empty() {
                    return Err(KeyPathError::new("path cannot end with '.'"));
                }
                if s.starts_with('.') {
                    return Err(KeyPathError::new("empty segment (consecutive dots)"));
                }
                if s.starts_with('[') {
                    return Err(KeyPathError::new(
                        "unexpected '.' before '['; use 'a[0]' not 'a.[0]'",
                    ));
                }
            }
            // If starts with '[', next iteration handles it directly
        }

        Ok(KeyPath { segs })
    }
}

// ---------------------------------------------------------------------------------------------- //
// TryIntoKeyPath

/// Trait for types that can be converted into a [`KeyPath`].
///
/// This enables flexible path arguments in traversal methods. String types
/// are parsed using the dotted path syntax, while [`KeyPath`] values pass
/// through directly.
pub trait TryIntoKeyPath {
    /// Attempts to convert this value into a [`KeyPath`].
    fn try_into_key_path(self) -> Result<KeyPath, KeyPathError>;
}

impl TryIntoKeyPath for &str {
    fn try_into_key_path(self) -> Result<KeyPath, KeyPathError> {
        self.parse()
    }
}

impl TryIntoKeyPath for String {
    fn try_into_key_path(self) -> Result<KeyPath, KeyPathError> {
        self.parse()
    }
}

impl TryIntoKeyPath for &String {
    fn try_into_key_path(self) -> Result<KeyPath, KeyPathError> {
        self.parse()
    }
}

impl TryIntoKeyPath for KeyPath {
    fn try_into_key_path(self) -> Result<KeyPath, KeyPathError> {
        Ok(self)
    }
}

impl TryIntoKeyPath for &KeyPath {
    fn try_into_key_path(self) -> Result<KeyPath, KeyPathError> {
        Ok(self.clone())
    }
}

impl TryIntoKeyPath for usize {
    fn try_into_key_path(self) -> Result<KeyPath, KeyPathError> {
        Ok(KeyPath::from_index(self))
    }
}

impl<const N: usize> TryIntoKeyPath for [&str; N] {
    fn try_into_key_path(self) -> Result<KeyPath, KeyPathError> {
        Ok(KeyPath::from_keys(self))
    }
}

impl TryIntoKeyPath for &[&str] {
    fn try_into_key_path(self) -> Result<KeyPath, KeyPathError> {
        Ok(KeyPath::from_keys(self.iter().copied()))
    }
}

impl TryIntoKeyPath for Vec<&str> {
    fn try_into_key_path(self) -> Result<KeyPath, KeyPathError> {
        Ok(KeyPath::from_keys(self))
    }
}

impl TryIntoKeyPath for Vec<String> {
    fn try_into_key_path(self) -> Result<KeyPath, KeyPathError> {
        Ok(KeyPath::from_keys(self))
    }
}

// ---------------------------------------------------------------------------------------------- //
// Syntax Helpers

/// Checks if a string is a valid bare identifier in path syntax.
pub fn is_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    match chars.next().unwrap() {
        '_' | 'a'..='z' | 'A'..='Z' => (),
        _ => return false,
    }
    chars.all(|c| matches!(c, '_' | 'a'..='z' | 'A'..='Z' | '0'..='9'))
}

/// Quotes a string with proper escaping for path syntax.
pub fn quote_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:04X}}}", c as u32);
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------------------------- //
// Tests

#[cfg(test)]
mod tests {
    use super::*;

    // Valid paths

    #[test]
    fn valid_single_key() {
        let kp: KeyPath = "a".parse().unwrap();
        assert_eq!(kp.to_string(), "a");
        assert_eq!(kp.to_string().parse::<KeyPath>().unwrap(), kp);
    }

    #[test]
    fn valid_dotted_path() {
        let kp: KeyPath = "a.b.c".parse().unwrap();
        assert_eq!(kp.to_string(), "a.b.c");
        assert_eq!(kp.to_string().parse::<KeyPath>().unwrap(), kp);
    }

    #[test]
    fn valid_key_with_index() {
        let kp: KeyPath = "a[0]".parse().unwrap();
        assert_eq!(kp.to_string(), "a[0]");
        assert_eq!(kp.to_string().parse::<KeyPath>().unwrap(), kp);
    }

    #[test]
    fn valid_mixed_path() {
        let kp: KeyPath = "a[0].b".parse().unwrap();
        assert_eq!(kp.to_string(), "a[0].b");
        assert_eq!(kp.to_string().parse::<KeyPath>().unwrap(), kp);
    }

    #[test]
    fn valid_quoted_key_with_dot() {
        let kp: KeyPath = r#"["a.b"]"#.parse().unwrap();
        // Display should quote keys containing dots, roundtrip should work
        assert_eq!(kp.to_string().parse::<KeyPath>().unwrap(), kp);
    }

    // Invalid paths

    fn assert_parse_error(input: &str, expected_substring: &str) {
        let err = input.parse::<KeyPath>().expect_err("expected error");
        assert!(
            err.to_string().contains(expected_substring),
            "error '{}' does not contain '{}'",
            err,
            expected_substring
        );
    }

    #[test]
    fn invalid_empty_string() {
        assert_parse_error("", "empty path");
    }

    #[test]
    fn invalid_leading_dot() {
        assert_parse_error(".a", "cannot start with '.'");
    }

    #[test]
    fn invalid_trailing_dot() {
        assert_parse_error("a.", "cannot end with '.'");
    }

    #[test]
    fn invalid_consecutive_dots() {
        assert_parse_error("a..b", "empty segment");
    }

    #[test]
    fn invalid_dot_before_bracket() {
        assert_parse_error("a.[0]", "unexpected '.' before '['");
    }

    #[test]
    fn invalid_unterminated_bracket() {
        assert_parse_error("[\"unterminated", "unterminated string");
    }

    #[test]
    fn invalid_unquoted_whitespace() {
        assert_parse_error("a b", "invalid identifier");
    }

    #[test]
    fn invalid_unquoted_whitespace_in_path() {
        assert_parse_error("a b.c", "invalid identifier");
    }

    #[test]
    fn invalid_unquoted_hyphen() {
        assert_parse_error("foo-bar", "invalid identifier");
    }
}
