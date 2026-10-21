//! Demonstrates array replacement, flat append, grouped append, and positional arrays.
//!
//! Run with: cargo run --example multi_values -- --help
//! Or: cargo run --example multi_values -- --crop 0 0 640 480 --point 10 20 --tag cat dog -- a.png b.png

use std::path::PathBuf;

use scry::cli::setup::{ConfigSource, Setup, SetupError};
use scry::Config;

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config)]
struct Inputs {
    /// Crop bounds in left, top, right, bottom order.
    crop: Option<[u32; 4]>,
    /// Tags appended to any tags loaded from config.
    #[scry(default = Vec::new())]
    tags: Vec<String>,
    /// Coordinate pairs appended to any points loaded from config.
    #[scry(default = Vec::new())]
    points: Vec<(i32, i32)>,
    /// Input paths replacing any paths loaded from config.
    #[scry(default = Vec::new())]
    paths: Vec<PathBuf>,
}

fn main() -> Result<(), SetupError> {
    Setup::standard("multi-values")
        .about("Collects arrays and appends values before querying or parsing config.")
        .config_source(|_| {
            ConfigSource::new().option("config", Some('C'), "Config file.").or_empty()
        })
        .expose(|e| {
            e.option("crop").array(4).value_names(["LEFT", "TOP", "RIGHT", "BOTTOM"]);
            e.option("tags").append().num_args(1..).long("tag").short('t').value_name("TAG");
            e.option("points").array(2).append().long("point").value_names(["X", "Y"]);
            e.positional("paths").array(1..).value_name("PATH");
        })
        .into_bundle(|inputs: Inputs| {
            println!("Crop: {:?}", inputs.crop);
            println!("Tags: {:?}", inputs.tags);
            println!("Points: {:?}", inputs.points);
            println!("Paths: {:?}", inputs.paths);
        })
        .run()?;
    Ok(())
}
