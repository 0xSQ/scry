//! Demonstrates command-tree discovery, executable parents, and source metadata.
//!
//! Runs with `cargo run --example command_configs -- catalog -S desc`.
//! Looks under `~/.config/atlas/` when no explicit `-C` file is supplied.
//! Loads a whole explicit leaf payload with `catalog -C catalog.rhai`.
//! Selects the parent payload with `archive -C 'archive.rhai::["$self"]'`.
//! Selects a child payload with `archive prune -C archive.rhai::prune`.
//! Loads optional `machine.*` as `host` in Rhai command scripts and their imports.

use scry::cli::setup::{ApplicationConfig, CommandConfigSource, Setup, SetupError};
use scry::cli::Bundle;
use scry::Config;

// ---------------------------------------------------------------------------------------------- //

fn main() -> Result<(), SetupError> {
    let application = ApplicationConfig::new("atlas")
        .environment_file("machine")
        .environment_binding("host")
        .rhai_engine(|engine, _environment| {
            engine.register_fn("default_retention_days", || 30_i64);
        });
    let catalog = setup("catalog", &application).into_bundle_with_source(
        |config: Catalog, _matches, source| {
            print_source(source);
            println!("Categories: {:?}", config.categories);
        },
    );
    let create = setup("create", &application).into_bundle(|config: Create| {
        println!("Compress archive: {}", config.compress);
    });
    let prune = setup("prune", &application).into_bundle(|config: Prune| {
        println!("Keep archives for {} days", config.keep_days);
    });
    let archive = setup("archive", &application)
        .into_bundle(|config: Archive| println!("Verbose archive summary: {}", config.verbose))
        .with_subcommands(vec![create, prune]);

    Bundle::group("atlas", "Organizes documents.", vec![catalog, archive]).run()?;
    Ok(())
}

#[derive(Debug, Config)]
struct Catalog {
    /// Categories to include in the catalog.
    #[scry(default = Vec::new())]
    categories: Vec<String>,
}

#[derive(Debug, Config)]
struct Archive {
    /// Includes additional archive summary details.
    #[scry(default = false)]
    verbose: bool,
}

#[derive(Debug, Config)]
struct Create {
    /// Compresses the archive contents.
    #[scry(default = true)]
    compress: bool,
}

#[derive(Debug, Config)]
struct Prune {
    /// Retains archives newer than this number of days.
    #[scry(default = 30)]
    keep_days: u32,
}

fn setup(name: &str, application: &ApplicationConfig) -> Setup {
    Setup::standard(name).config_source(|source| source.application(application.clone()).or_empty())
}

fn print_source(source: Option<&CommandConfigSource>) {
    if let Some(source) = source {
        println!("Config: {}", source.logical_path.display());
        if !source.selection.is_empty() {
            println!("Selection: {}", source.selection);
        }
        println!("Physical source: {}", source.physical_path.display());
    } else {
        println!("Config: defaults and CLI input");
    }
}
