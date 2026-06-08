//! Comprehensive example demonstrating CLI composing styles.
//!
//! This example shows different ways to use the CLI setup module:
//! - Fallible handlers returning `anyhow::Result<()>` (most common)
//! - Infallible handlers returning `()`
//! - Custom error types in handler returns
//! - Parent/child command composition
//!
//! Run with: cargo run --example cli_bundling_styles -- --help
//! Or: cargo run --example cli_bundling_styles -- serve examples/server.json
//! Or: cargo run --example cli_bundling_styles -- check examples/check.json
//! Or: cargo run --example cli_bundling_styles -- migrate examples/server.json

use std::error::Error;
use std::fmt;
use std::io;

use scry::cli::setup::{Setup, SetupError};
use scry::cli::Bundle;
use scry::Config;

// ---------------------------------------------------------------------------------------------- //
// Config Types

/// Server configuration.
#[derive(Debug, Config)]
struct ServerConfig {
    /// Server hostname.
    host: String,
    /// Server port.
    #[scry(default = 8080)]
    port: u16,
}

/// Check configuration.
#[derive(Debug, Config)]
struct CheckConfig {
    /// Path to validate.
    path: String,
    /// Enable strict mode.
    #[scry(default = false)]
    strict: bool,
}

// ---------------------------------------------------------------------------------------------- //
// Custom Error Types

/// Error for the serve command.
#[derive(Debug)]
#[allow(dead_code)]
enum ServeError {
    InvalidPort(u16),
    BindFailed(io::Error),
}

impl fmt::Display for ServeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServeError::InvalidPort(port) => write!(f, "invalid port: {}", port),
            ServeError::BindFailed(e) => write!(f, "failed to bind: {}", e),
        }
    }
}

impl Error for ServeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ServeError::BindFailed(e) => Some(e),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Handlers

/// Serve handler - returns a custom error type.
fn serve_handler(config: ServerConfig) -> Result<(), ServeError> {
    if config.port == 0 {
        return Err(ServeError::InvalidPort(config.port));
    }
    println!("Starting server at {}:{}", config.host, config.port);
    Ok(())
}

/// Check handler - infallible (returns unit).
fn check_handler(config: CheckConfig) {
    println!("Checking path: {}", config.path);
    if config.strict {
        println!("  (strict mode enabled)");
    }
}

/// Status handler - returns io::Error.
fn status_handler(_config: ServerConfig) -> Result<(), io::Error> {
    println!("Server status: running");
    Ok(())
}

/// Migrate handler - returns anyhow::Result (most common pattern).
fn migrate_handler(config: ServerConfig) -> anyhow::Result<()> {
    println!("Migrating server at {}:{}", config.host, config.port);
    // In a real app, you could use anyhow's context methods:
    // some_operation().context("failed to migrate")?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------- //
// Building the CLI with anyhow::Result<()>
//
// The recommended pattern is for all handlers to return the same type - typically
// `anyhow::Result<()>`. This allows composing them into parent commands.

fn build_anyhow_cli() -> Bundle<Option<anyhow::Result<()>>, SetupError> {
    // Serve command - wrap ServeError in anyhow
    let serve = Setup::new("serve").about("Start the server").into_bundle(
        |config: ServerConfig| -> anyhow::Result<()> {
            serve_handler(config)?;
            Ok(())
        },
    );

    // Check command - infallible, wrap in Ok(())
    let check = Setup::new("check").about("Check configuration").into_bundle(
        |config: CheckConfig| -> anyhow::Result<()> {
            check_handler(config);
            Ok(())
        },
    );

    // Status command - wrap io::Error in anyhow
    let status = Setup::new("status").about("Show server status").into_bundle(
        |config: ServerConfig| -> anyhow::Result<()> {
            status_handler(config)?;
            Ok(())
        },
    );

    // Migrate command - already returns anyhow::Result
    let migrate =
        Setup::new("migrate").about("Run database migrations").into_bundle(migrate_handler);

    // Parent command grouping all subcommands
    Bundle::group("server", "Server management tool", vec![serve, check, status, migrate])
}

// ---------------------------------------------------------------------------------------------- //
// Building the CLI with () (infallible handlers)
//
// When all handlers are infallible, the CLI returns Option<()>.

#[allow(dead_code)]
fn build_infallible_cli() -> Bundle<Option<()>, SetupError> {
    let check = Setup::new("check").about("Check configuration").into_bundle(check_handler);

    Bundle::group("tools", "Tool commands", vec![check])
}

// ---------------------------------------------------------------------------------------------- //
// Error Handling Pattern
//
// Execution errors (config loading, CLI parsing) are SetupError.
// User/domain errors live inside the handler's return type (e.g., anyhow::Result).

#[allow(dead_code)]
fn handle_cli_result(result: Result<Option<anyhow::Result<()>>, SetupError>) {
    match result {
        Ok(Some(Ok(()))) => {
            // Handler ran successfully
        }
        Ok(Some(Err(user_err))) => {
            // Handler ran but returned an error
            eprintln!("Error: {}", user_err);
            std::process::exit(1);
        }
        Ok(None) => {
            // Support command handled the request (--desc, --get, etc.)
        }
        Err(e) => {
            // Setup/pipeline error (config loading, CLI parsing, etc.)
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Main

fn main() -> anyhow::Result<()> {
    // run() returns Result<Payload<anyhow::Result<()>>, SetupError>
    // - Err(e): pipeline error (config loading, CLI parsing)
    // - Ok(Payload::Skipped): support command handled (--desc, --get, etc.)
    // - Ok(Payload::Executed(result)): handler ran and returned result
    if let Some(handler_result) = build_anyhow_cli().run()? {
        handler_result?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------- //
// Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anyhow_cli_builds() {
        let _bundle = build_anyhow_cli();
    }

    #[test]
    fn infallible_cli_builds() {
        let _bundle = build_infallible_cli();
    }
}
