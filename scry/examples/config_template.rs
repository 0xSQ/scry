//! Generates an automatic configuration scaffold without constructing a settings value.
//!
//! Run with `cargo run --example config_template -- -S template`.
//! Compare `--mode ref` and `--mode explicit`, or select `--as json5` when enabled.
//! Adjust presentation with `--spacing compact`, `--type-hints inline`, or `--type-hints none`.

#![allow(dead_code)]

use std::path::PathBuf;

use scry::cli::setup::{Setup, SetupError};
use scry::Config;

// ---------------------------------------------------------------------------------------------- //

/// Configures an image export command.
#[derive(Config)]
struct Settings {
    /// Names the source image.
    input: PathBuf,
    /// Selects the output settings.
    output: Output,
    /// Adds labels to the exported image.
    #[scry(default = Vec::new())]
    labels: Vec<String>,
    /// Enables an additional thumbnail export.
    thumbnail: Option<Thumbnail>,
    /// Prints processing details.
    #[scry(default = false)]
    verbose: bool,
}

#[derive(Config)]
struct Output {
    /// Selects the output format.
    #[scry(default = "jpeg".to_owned())]
    format: String,
    /// Sets JPEG quality from 1 to 100.
    #[scry(default = 90)]
    quality: u32,
    /// Copies embedded metadata to the exported image.
    #[scry(default = true)]
    preserve_metadata: bool,
}

#[derive(Config)]
struct Thumbnail {
    /// Names the thumbnail destination.
    path: PathBuf,
    /// Limits the longest thumbnail side in pixels.
    #[scry(default = 256)]
    size: u32,
}

fn main() -> Result<(), SetupError> {
    Setup::new("image-export").into_bundle(|_: Settings| ()).run()?;
    Ok(())
}
