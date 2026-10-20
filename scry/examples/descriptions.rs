//! Displays configuration descriptions and queries nested fields.
//!
//! Run with: cargo run --example descriptions

// This example describes types without constructing their values.
#![allow(dead_code)]

use std::path::PathBuf;

use scry::kit::KeyValues;
use scry::Describe;

// ---------------------------------------------------------------------------------------------- //

/// Describes the inputs of a processing command.
#[derive(Describe)]
struct Settings {
    /// Enables processing.
    enabled: bool,
    /// Names this configuration.
    name: String,
    /// Selects the operating mode.
    mode: Mode,
    /// Groups input settings.
    items: Vec<Input>,
    /// Associates labels with numeric values.
    labels: KeyValues<i32>,
    /// Pairs a name with a priority.
    pair: Pair,
    /// Names the output path.
    output: OutputPath,
}

/// Selects automatic, fixed, range, or named processing.
#[derive(Describe)]
enum Mode {
    /// Chooses parameters automatically.
    Auto,
    /// Uses one fixed value.
    Fixed(u32),
    /// Uses two range endpoints.
    Range(i32, i32),
    /// Selects a named configuration.
    Named {
        /// Names the configuration.
        name: String,
        /// Enables this configuration.
        enabled: bool,
    },
}

#[derive(Describe)]
struct Input {
    /// Adds an optional note.
    note: Option<String>,
    /// Sets the number of retry attempts.
    #[scry(default = 10)]
    retries: u32,
    /// Selects a window by its endpoints.
    window: (u32, u32),
    /// Supplies red, green, and blue components.
    rgb: [u8; 3],
}

/// Pairs a name with a priority.
#[derive(Describe)]
struct Pair(String, i32);

/// Names the output path.
#[derive(Describe)]
struct OutputPath(PathBuf);

fn main() {
    let description = Settings::describe();
    println!("{}", description.display());

    for path in [
        "enabled",
        "items[0].retries",
        "pair[1]",
        "pair[5]",
        "missing",
    ] {
        match description.validate_path(path) {
            Ok(()) => println!("'{path}' is valid."),
            Err(error) => println!("{error}"),
        }
    }

    if let Some(entry) = description.entry_at_path("pair[0]") {
        println!("\nDescription of pair[0]:\n{}", entry.display());
    }
}
