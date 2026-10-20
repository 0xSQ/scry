//! Composes commands with custom-error, anyhow, and infallible handlers.
//!
//! Run from the repository root:
//!   cargo run --example cli_bundling_styles -- --help
//!   cargo run --example cli_bundling_styles -- serve -C scry/examples/server.json
//!   cargo run --example cli_bundling_styles -- check -C scry/examples/check.json

use scry::cli::setup::{Setup, SetupError};
use scry::cli::Bundle;
use scry::Config;

// ---------------------------------------------------------------------------------------------- //

fn main() -> anyhow::Result<()> {
    // Setup errors are the outer Result. A support action returns None.
    // Executed handlers return Some, containing their own result.
    if let Some(result) = build_cli().run()? {
        result?;
    }
    Ok(())
}

fn build_cli() -> Bundle<Option<anyhow::Result<()>>, SetupError> {
    // Grouped handlers share one return type. Convert custom errors at this boundary.
    let serve = Setup::standard("serve").about("Starts the server.").into_bundle(
        |config: ServerConfig| -> anyhow::Result<()> {
            serve(config)?;
            Ok(())
        },
    );

    // An infallible handler can participate by returning Ok after it runs.
    let check = Setup::standard("check").about("Checks a path.").into_bundle(
        |config: CheckConfig| -> anyhow::Result<()> {
            check(config);
            Ok(())
        },
    );

    Bundle::group("server", "Manages a server.", vec![serve, check])
}

#[derive(Debug, Config)]
struct ServerConfig {
    /// Server hostname.
    host: String,
    /// Server port.
    #[scry(default = 8080)]
    port: u16,
}

fn serve(config: ServerConfig) -> Result<(), InvalidPort> {
    if config.port == 0 {
        return Err(InvalidPort);
    }
    println!("Starting server at {}:{}", config.host, config.port);
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error("server port must be nonzero")]
struct InvalidPort;

#[derive(Debug, Config)]
struct CheckConfig {
    /// Path to validate.
    path: String,
    /// Enables strict mode.
    #[scry(default = false)]
    strict: bool,
}

fn check(config: CheckConfig) {
    println!("Checking path: {}", config.path);
    if config.strict {
        println!("  (strict mode enabled)");
    }
}
