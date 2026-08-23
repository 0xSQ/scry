//! Deploy example - demonstrates building a config-driven CLI with Scry.
//!
//! Run with: cargo run --example deploy -- --help
//! Or: cargo run --example deploy -- examples/deploy.json
#![allow(dead_code)]

use scry::cli::setup::{Setup, SetupError};
use scry::Config;

#[derive(Debug, Config)]
struct Deploy {
    /// Target server hostname.
    target: String,
    /// Deployment environment (e.g. "dev", "staging", "prod").
    environment: String,
    /// Number of retry attempts on failure.
    #[scry(default = 3)]
    retries: u32,
    /// Request timeout in seconds.
    timeout_secs: Option<u64>,
    /// Notification settings.
    notify: Option<Notify>,
    /// Where to send log output.
    #[scry(from_defaults)]
    log_output: LogOutput,
    /// Run without making changes.
    #[scry(default = false)]
    dry_run: bool,
}

#[derive(Debug, Config)]
enum LogOutput {
    /// Log to standard output.
    #[scry(default)]
    Stdout,
    /// Log to standard error.
    Stderr,
}

#[derive(Debug, Config)]
struct Notify {
    /// Slack URL to send notifications to.
    slack_url: String,
    /// Only notify on failure.
    #[scry(default = false)]
    on_failure_only: bool,
}

fn deploy(config: Deploy) {
    if config.dry_run {
        println!("Performing dry run...");
    }
    println!("Deploying to: {}", config.target);
    println!("Environment: {}", config.environment);
    println!("Retries: {}", config.retries);
    if let Some(timeout) = config.timeout_secs {
        println!("Timeout: {}s", timeout);
    }
    if let Some(notify) = config.notify {
        println!("Sending notifications to: {}", notify.slack_url);
        if notify.on_failure_only {
            println!("   (only on failure)");
        }
    }
}

fn main() -> Result<(), SetupError> {
    Setup::standard("deploy")
        .expose(|e| {
            e.option("environment").short('e').long("env");
            e.flag("dry_run", true).short('n');
        })
        .into_bundle(deploy)
        .run()?;
    Ok(())
}
