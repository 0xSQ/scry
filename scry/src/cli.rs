//! CLI utilities for building command-line applications.
//!
//! This module provides [`Bundle`] for pairing clap commands with handler functions,
//! as well as the [`setup`] submodule for utilities for building config-based CLI commands.

mod bundle;
pub mod setup;

pub use bundle::{Bundle, Dispatcher};
