//! Demonstrates custom input preparation before Setup applies CLI edits.
//!
//! The loader parses a config file into a Node, then applies the optional `DEPLOY_ENV`
//! environment variable. Exposed arguments and input edits take precedence over that
//! prepared input. `-S get` shows the prepared, edited Node before typed conversion.
//!
//! Help and descriptions do not run the loader.
//!
//! Run with:
//!   cargo run --example manual_setup -- -C examples/manual_setup.json
//!   cargo run --example manual_setup -- -S desc
//!   cargo run --example manual_setup -- -C examples/manual_setup.json -S get
//!   cargo run --example manual_setup -- -C examples/manual_setup.json -S --set retries 5
//!   cargo run --example manual_setup -- -C examples/manual_setup.json --env staging -n
//!   cargo run --example manual_setup -- -C examples/manual_setup.json --canary beta

use std::path::Path;

use scry::cli::setup::Setup;
use scry::{Config, Describe, FromNode, Node, ToNode};

// ---------------------------------------------------------------------------------------------- //
// Config Type

/// Server deployment configuration.
#[derive(Debug, Config)]
struct Deploy {
    /// Target server hostname.
    target: String,
    /// Deployment environment (e.g. "dev", "staging", "prod").
    environment: String,
    /// Number of retry attempts on failure.
    #[scry(default = 3)]
    retries: u32,
    /// Run without making changes.
    #[scry(default = false)]
    dry_run: bool,
    /// Rollout strategy. Full rollout when unset.
    rollout: Option<Rollout>,
}

/// How the deployment rolls out.
///
/// Serializes in Scry's standard enum form, a single-key map naming the variant:
/// `{ "canary": "beta" }` or `{ "percent": 25 }`.
#[derive(Debug, FromNode, ToNode, Describe)]
enum Rollout {
    /// Deploys to a named canary group first.
    Canary(String),
    /// Deploys to a percentage of servers.
    Percent(u32),
}

// ---------------------------------------------------------------------------------------------- //
// Main

fn main() -> anyhow::Result<()> {
    Setup::standard("deploy")
        .about("Deploys using file, environment, and command-line input.")
        .config_source(|source| source.loader(load_deploy_input))
        .expose(|expose| {
            expose.option("environment").short('e').long("env");
            expose.flag("dry_run", true).short('n');
            expose.option("rollout").variant("canary");
            expose.option("rollout").variant("percent");
        })
        .into_bundle(deploy)
        .run()?;
    Ok(())
}

fn load_deploy_input(path: &Path) -> anyhow::Result<Node> {
    let mut node = Node::parse_file(path)?;
    match std::env::var("DEPLOY_ENV") {
        Ok(environment) => {
            node.set_value("environment", environment)?;
        }
        Err(std::env::VarError::NotPresent) => {}
        Err(error) => return Err(error.into()),
    }
    Ok(node)
}

fn deploy(config: Deploy) {
    if config.dry_run {
        println!("Performing dry run...");
    }
    println!("Deploying to: {}", config.target);
    println!("Environment: {}", config.environment);
    println!("Retries: {}", config.retries);
    match &config.rollout {
        Some(Rollout::Canary(group)) => println!("Rollout: canary group '{group}'"),
        Some(Rollout::Percent(percent)) => println!("Rollout: {percent}% of servers"),
        None => println!("Rollout: full"),
    }
}
