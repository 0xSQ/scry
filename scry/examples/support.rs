//! Demonstrates configuration support in Setup commands.
//!
//! Examples:
//! `cargo run --example support -- -S desc`
//! `cargo run --example support -- hello -S get`
//! `cargo run --example support -- hello -S --set count 2 check`
//! `cargo run --example support -- hello -S --set count 2`

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
    Setup::new("support")
        .config_source(|_| {
            ConfigSource::new()
                .option("config", Some('C'), "Loads an optional configuration file.")
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
