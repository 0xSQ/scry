use std::fmt;

// ---------------------------------------------------------------------------------------------- //

/// Reports that a string did not match a generated enum variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringEnumError {
    type_name: &'static str,
    value: String,
    expected: &'static str,
}

impl StringEnumError {
    /// Creates a string enum parse error.
    pub fn new(type_name: &'static str, value: impl Into<String>, expected: &'static str) -> Self {
        Self {
            type_name,
            value: value.into(),
            expected,
        }
    }
}

impl fmt::Display for StringEnumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid {} value '{}' - expected one of: {}",
            self.type_name, self.value, self.expected
        )
    }
}

impl std::error::Error for StringEnumError {}
