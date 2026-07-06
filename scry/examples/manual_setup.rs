//! Manual CLI setup example - using command and setup modules directly.
//!
//! This example demonstrates how to construct a config-driven CLI **without** using the
//! high-level `Setup` API. Instead, it uses the lower-level building blocks from the
//! `command` and `setup` modules.
//!
//! Use this approach when you need:
//! - Custom config loading (e.g., your own Rhai engine with registered functions)
//! - Non-file config sources (e.g., embedded defaults, environment variables)
//! - Fine-grained control over the argument parsing and config preparation flow
//! - Custom error handling or logic between setup stages
//!
//! Run with:
//!   cargo run --example manual_setup -- examples/manual_setup.json
//!   cargo run --example manual_setup -- examples/manual_setup.json --desc
//!   cargo run --example manual_setup -- examples/manual_setup.json --get
//!   cargo run --example manual_setup -- examples/manual_setup.json --set retries 5
//!   cargo run --example manual_setup -- examples/manual_setup.json --env staging -n
//!   cargo run --example manual_setup -- examples/manual_setup.json --canary beta

use clap::Command;

use scry::cli::setup::check_collisions;
use scry::cli::setup::{
    ConfigSource, ExposeMap, OverrideArgs, QueryArgs, Required, ResolvedConfigInput,
};
use scry::{Config, Describe, Node};

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
    /// Rollout strategy; full rollout when unset.
    rollout: Option<Rollout>,
}

/// How the deployment rolls out.
///
/// Serializes in Scry's standard enum form, a single-key map naming the variant:
/// `{"canary": "beta"}` or `{"percent": 25}`.
#[derive(Debug, scry::FromNode, scry::ToNode, scry::Describe)]
enum Rollout {
    /// Deploys to a named canary group first.
    Canary(String),
    /// Deploys to a percentage of servers.
    Percent(u32),
}

// ---------------------------------------------------------------------------------------------- //
// Main

fn main() -> anyhow::Result<()> {
    // Set the specs for how we want to assemble our clap command.
    let config_source =
        ConfigSource::new().positional("CONFIG", "Path to deployment config file", Required::Yes);
    let override_args = OverrideArgs::standard();
    let query_args = QueryArgs::standard();
    let mut expose_map = ExposeMap::new();
    expose_map.option("environment").short('e').long("env");
    expose_map.flag("dry_run", true).short('n');
    // Variant options address one enum arm each; the long names derive from the variant keys,
    // and selecting one arm replaces whichever arm the config held.
    expose_map.option("rollout").variant("canary");
    expose_map.option("rollout").variant("percent");

    // Check for argument collisions.
    check_collisions(Some(&config_source), &override_args, &query_args, &expose_map, &[])?;

    // Set up the base command.
    let mut cmd = Command::new("deploy").about("Deploy to a server");

    // Augment it based on our spec data.
    cmd = config_source.augment(cmd, &query_args);
    cmd = override_args.augment(cmd);
    cmd = query_args.augment(cmd);
    cmd = expose_map.augment(cmd, &Deploy::describe());

    // Now parse the arguments into matches.
    let matches = cmd.get_matches();

    // Start by handling desc options if present (where we don't need the config file yet).
    if let Some(output) = query_args.check_desc::<Deploy>(&matches)? {
        println!("{}", output);
        return Ok(());
    }

    // We need the config file path to proceed, so load it into a Node.
    let mut node = match config_source.resolve_input("deploy", &matches)? {
        ResolvedConfigInput::Path(path) => Node::parse_file(&path)?,
        ResolvedConfigInput::Empty => Node::default(),
    };

    // Apply all override operations (set, remove, exposed options, and flags).
    override_args.apply::<Deploy>(&mut node, &expose_map, &matches)?;

    if let Some(output) = query_args.check_get::<Deploy>(&node, &matches)? {
        println!("{}", output);
        return Ok(());
    }

    // Finally, convert the node into our target type.
    let config: Deploy = node.as_type()?;

    // Do the thing!
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

    Ok(())
}
