//! Provides reusable color values and configuration notation for byte-based sRGB colors.
//!
#![doc = include_str!("../../docs/kit/colors.md")]
#![warn(missing_docs)]

mod adapters;
pub mod named;
mod scry;

use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use thiserror::Error;

pub use adapters::{rgb8_array, rgb8_tuple, rgba8_array, rgba8_tuple};

// ---------------------------------------------------------------------------------------------- //

/// Stores an opaque sRGB color with three byte channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb8([u8; 3]);

impl Rgb8 {
    /// Creates an opaque color from red, green, and blue bytes.
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self([red, green, blue])
    }

    /// Returns the red channel.
    pub const fn red(self) -> u8 {
        self.0[0]
    }

    /// Returns the green channel.
    pub const fn green(self) -> u8 {
        self.0[1]
    }

    /// Returns the blue channel.
    pub const fn blue(self) -> u8 {
        self.0[2]
    }

    /// Borrows the channels in RGB order.
    pub const fn as_array(&self) -> &[u8; 3] {
        &self.0
    }

    /// Returns the channels in RGB order.
    pub const fn into_array(self) -> [u8; 3] {
        self.0
    }
}

impl FromStr for Rgb8 {
    type Err = ParseError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        Rgba8::from_str(source)?.try_into().map_err(Into::into)
    }
}

impl Display for Rgb8 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let [red, green, blue] = self.0;
        write!(formatter, "#{red:02x}{green:02x}{blue:02x}")
    }
}

impl From<[u8; 3]> for Rgb8 {
    fn from(channels: [u8; 3]) -> Self {
        Self(channels)
    }
}

impl From<Rgb8> for [u8; 3] {
    fn from(color: Rgb8) -> Self {
        color.into_array()
    }
}

impl From<(u8, u8, u8)> for Rgb8 {
    fn from((red, green, blue): (u8, u8, u8)) -> Self {
        Self::new(red, green, blue)
    }
}

impl From<Rgb8> for (u8, u8, u8) {
    fn from(color: Rgb8) -> Self {
        let [red, green, blue] = color.into_array();
        (red, green, blue)
    }
}

impl TryFrom<Rgba8> for Rgb8 {
    type Error = NonOpaqueError;

    fn try_from(color: Rgba8) -> Result<Self, Self::Error> {
        let [red, green, blue, alpha] = color.into_array();
        if alpha != 255 {
            return Err(NonOpaqueError { alpha });
        }
        Ok(Self::new(red, green, blue))
    }
}

/// Stores an sRGB color with separate byte alpha in RGBA order.
///
/// Alpha 0 is fully transparent and alpha 255 is fully opaque. RGB channels are not
/// premultiplied and remain intact even when alpha is zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba8([u8; 4]);

impl Rgba8 {
    /// Creates a color from red, green, blue, and separate alpha bytes.
    pub const fn new(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self([red, green, blue, alpha])
    }

    /// Returns the red channel without premultiplication.
    pub const fn red(self) -> u8 {
        self.0[0]
    }

    /// Returns the green channel without premultiplication.
    pub const fn green(self) -> u8 {
        self.0[1]
    }

    /// Returns the blue channel without premultiplication.
    pub const fn blue(self) -> u8 {
        self.0[2]
    }

    /// Returns alpha, where 0 is transparent and 255 is opaque.
    pub const fn alpha(self) -> u8 {
        self.0[3]
    }

    /// Borrows the channels in RGBA order with separate alpha.
    pub const fn as_array(&self) -> &[u8; 4] {
        &self.0
    }

    /// Returns the channels in RGBA order with separate alpha.
    pub const fn into_array(self) -> [u8; 4] {
        self.0
    }
}

impl FromStr for Rgba8 {
    type Err = ParseError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let source = source.trim_matches(|character: char| character.is_ascii_whitespace());
        if source.is_empty() {
            return Err(ParseError::EmptyInput);
        }
        if let Some(digits) = source.strip_prefix('#') {
            return parse_hex(digits);
        }
        if source.eq_ignore_ascii_case("transparent") {
            return Ok(Self::new(0, 0, 0, 0));
        }
        named::named_rgb(source)
            .map(|channels| Rgb8::from(channels).into())
            .ok_or(ParseError::UnknownName)
    }
}

impl Display for Rgba8 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let [red, green, blue, alpha] = self.0;
        write!(formatter, "#{red:02x}{green:02x}{blue:02x}")?;
        if alpha != 255 {
            write!(formatter, "{alpha:02x}")?;
        }
        Ok(())
    }
}

impl From<[u8; 4]> for Rgba8 {
    fn from(channels: [u8; 4]) -> Self {
        Self(channels)
    }
}

impl From<Rgba8> for [u8; 4] {
    fn from(color: Rgba8) -> Self {
        color.into_array()
    }
}

impl From<(u8, u8, u8, u8)> for Rgba8 {
    fn from((red, green, blue, alpha): (u8, u8, u8, u8)) -> Self {
        Self::new(red, green, blue, alpha)
    }
}

impl From<Rgba8> for (u8, u8, u8, u8) {
    fn from(color: Rgba8) -> Self {
        let [red, green, blue, alpha] = color.into_array();
        (red, green, blue, alpha)
    }
}

impl From<Rgb8> for Rgba8 {
    fn from(color: Rgb8) -> Self {
        let [red, green, blue] = color.into_array();
        Self::new(red, green, blue, 255)
    }
}

/// Reports why a color string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseError {
    /// The string contains no color after trimming ASCII whitespace.
    #[error("color string is empty")]
    EmptyInput,
    /// Hex notation has an unsupported number of bytes after the prefix.
    #[error("hex colors require 3, 4, 6, or 8 digits after # (received {digits})")]
    InvalidHexLength {
        /// The number of bytes following the hex prefix.
        digits: usize,
    },
    /// Hex notation contains a byte outside the ASCII hexadecimal alphabet.
    #[error("hex colors must contain only ASCII hexadecimal digits after #")]
    InvalidHexDigit,
    /// The string is neither a CSS color name nor supported prefixed hex notation.
    #[error("expected a CSS color name or a #RGB, #RGBA, #RRGGBB, or #RRGGBBAA hex color")]
    UnknownName,
    /// The color has transparency where an opaque color is required.
    #[error(transparent)]
    NonOpaque(#[from] NonOpaqueError),
}

/// Reports an attempt to convert a nonopaque color into opaque RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("opaque colors require alpha 255 (received {alpha})")]
pub struct NonOpaqueError {
    alpha: u8,
}

impl NonOpaqueError {
    /// Returns the alpha value that prevented conversion.
    pub const fn alpha(self) -> u8 {
        self.alpha
    }
}

// ---------------------------------------------------------------------------------------------- //

fn parse_hex(digits: &str) -> Result<Rgba8, ParseError> {
    let mut channels = [0, 0, 0, 255];
    match digits.len() {
        3 | 4 => {
            for (channel, digit) in channels.iter_mut().zip(digits.bytes()) {
                *channel = hex_digit(digit)? * 17;
            }
        }
        6 | 8 => {
            let (pairs, _) = digits.as_bytes().as_chunks::<2>();
            for (channel, pair) in channels.iter_mut().zip(pairs) {
                *channel = hex_digit(pair[0])? * 16 + hex_digit(pair[1])?;
            }
        }
        digits => return Err(ParseError::InvalidHexLength { digits }),
    }
    Ok(Rgba8::from(channels))
}

fn hex_digit(byte: u8) -> Result<u8, ParseError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(ParseError::InvalidHexDigit),
    }
}
