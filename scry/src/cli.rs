//! CLI utilities for building command-line applications.
//!
//! This module provides [`Bundle`] for pairing clap commands with handler functions,
//! as well as the [`setup`] submodule for utilities for building config-based CLI commands.
//!
//! [`Bundle::run`] and [`Bundle::run_from`] apply Clap's printing and exit policy. For tests or
//! embedding, [`Bundle::try_run`] and [`Bundle::try_run_from`] return [`RunError`] instead.
//! [`RunError::Parse`] retains ordinary parsing errors and successful help/version requests.
//! [`RunError::Handler`] retains the selected handler's error without converting it.
//!
//! With [`setup::Setup`], the handler error is [`setup::SetupError`]. Support-suffix parsing errors
//! remain [`RunError::Handler`] values containing [`setup::SetupError::Support`]. Successful
//! support actions retain their printed output and `None` result. Payload results stay unchanged.

mod bundle;
pub mod setup;

pub use bundle::{Bundle, RunError};
