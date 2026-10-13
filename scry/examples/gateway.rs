//! Demonstrates the flag-only Setup toolbox.
//!
//! Examples:
//! `cargo run --example gateway -- hello -S get`
//! `cargo run --example gateway -- hello -S --set count 2`
//! `cargo run --example gateway -- -S desc`

use anyhow::Result;
use scry::cli::setup::{ConfigSource, Setup};
use scry::Config;

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config)]
struct Input {
    /// Message supplied by the invocation.
    message: String,
    /// Number of times the message is printed.
    #[scry(default = 1)]
    count: usize,
}

fn main() -> Result<()> {
    Setup::new("gateway")
        .config_source(|_| {
            ConfigSource::new()
                .option("config-file", Some('C'), "Loads an optional configuration file.")
                .or_empty()
        })
        .expose(|expose| {
            expose.positional("message");
            expose.option("count");
        })
        .into_bundle(|input: Input| {
            for _ in 0..input.count {
                println!("{}", input.message);
            }
        })
        .run()?;
    Ok(())
}
