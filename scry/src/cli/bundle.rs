//! Provides `Bundle` for pairing clap commands with handlers.

use std::error::Error;
use std::fmt;

use clap::{ArgMatches, Command};

// ---------------------------------------------------------------------------------------------- //

/// A clap command paired with its handler and subcommand handlers.
///
/// Running the bundle consumes it and returns the selected handler's result.
pub struct Bundle<R, E> {
    command: Command,
    dispatcher: Dispatcher<R, E>,
}

impl<R: 'static, E: 'static> Bundle<R, E> {
    /// Creates a command with a handler that receives its parsed arguments.
    pub fn single(
        command: Command,
        handler: impl FnOnce(&ArgMatches) -> Result<R, E> + 'static,
    ) -> Self {
        Self::single_with_commands(command, move |matches, _| handler(matches))
    }

    /// Creates a handler that receives the command chain from the root to the selected command.
    pub(crate) fn single_with_commands(
        command: Command,
        handler: impl FnOnce(&ArgMatches, &[&Command]) -> Result<R, E> + 'static,
    ) -> Self {
        Self {
            command,
            dispatcher: Dispatcher {
                handler: Some(Box::new(handler)),
                children: Vec::new(),
            },
        }
    }

    /// Creates a command group that requires a subcommand unless a fallback is supplied.
    pub fn group(
        name: impl Into<String>,
        about: impl Into<String>,
        bundles: Vec<Bundle<R, E>>,
    ) -> Self {
        Self::group_with(Command::new(name.into()).about(about.into()), bundles)
    }

    /// Creates a command group from a configured clap command.
    pub fn group_with(command: Command, bundles: Vec<Bundle<R, E>>) -> Self {
        Self {
            command,
            dispatcher: Dispatcher {
                handler: None,
                children: Vec::new(),
            },
        }
        .with_subcommands(bundles)
    }

    /// Adds subcommands while retaining this command's existing handler and children.
    ///
    /// The command's own handler runs when no subcommand is selected.
    /// Commands without their own handler require a subcommand.
    pub fn with_subcommands(mut self, bundles: Vec<Bundle<R, E>>) -> Self {
        for bundle in bundles {
            let name = bundle.command.get_name().to_owned();
            self.command = self.command.subcommand(bundle.command);
            self.dispatcher.children.push((name, bundle.dispatcher));
        }
        self.command = self.command.subcommand_required(self.dispatcher.handler.is_none());
        self
    }

    /// Sets the handler used when no subcommand is selected.
    pub fn fallback(mut self, handler: impl FnOnce(&ArgMatches) -> Result<R, E> + 'static) -> Self {
        self.dispatcher.handler = Some(Box::new(move |matches, _| handler(matches)));
        self.command = self.command.subcommand_required(false);
        self
    }

    /// Parses the process arguments and runs the selected handler.
    ///
    /// Prints help, version output, or a parse error and exits with Clap's status when parsing
    /// stops execution. Handler errors are returned unchanged. The caller can use
    /// [`Self::try_run`] to handle parser outcomes without exiting.
    pub fn run(self) -> Result<R, E> {
        self.run_from(std::env::args_os())
    }

    /// Parses supplied arguments and runs the selected handler.
    ///
    /// Prints help, version output, or a parse error and exits with Clap's status when parsing
    /// stops execution. Handler errors are returned unchanged. The caller can use
    /// [`Self::try_run_from`] to handle parser outcomes without exiting.
    pub fn run_from<I, S>(self, args: I) -> Result<R, E>
    where
        I: IntoIterator<Item = S>,
        S: Into<std::ffi::OsString> + Clone,
    {
        match self.try_run_from(args) {
            Ok(value) => Ok(value),
            Err(RunError::Parse(error)) => error.exit(),
            Err(RunError::Handler(error)) => Err(error),
        }
    }

    /// Parses the process arguments and runs the selected handler without exiting on parser outcomes.
    ///
    /// Behaves like [`Self::try_run_from`] using the current process's native arguments.
    pub fn try_run(self) -> Result<R, RunError<E>> {
        self.try_run_from(std::env::args_os())
    }

    /// Parses supplied arguments and runs the selected handler without exiting on parser outcomes.
    ///
    /// Returns Clap's help, version, and invalid-input outcomes as [`RunError::Parse`]. The caller
    /// chooses whether to print them or exit, using their original exit status and output stream.
    /// Handler errors become [`RunError::Handler`] without changing the handler's return value.
    ///
    /// This does not capture output or prevent handlers from exiting or panicking. Configuration
    /// support actions keep their existing behavior, including printing help and other results.
    pub fn try_run_from<I, S>(mut self, args: I) -> Result<R, RunError<E>>
    where
        I: IntoIterator<Item = S>,
        S: Into<std::ffi::OsString> + Clone,
    {
        let matches = self.command.try_get_matches_from_mut(args).map_err(RunError::Parse)?;
        self.dispatcher.dispatch(&matches, vec![&self.command]).map_err(RunError::Handler)
    }

    /// Returns the underlying clap command for inspection.
    pub fn command(&self) -> &Command {
        &self.command
    }
}

/// A parser outcome or an error returned by a bundle's selected handler.
///
/// Parsing includes successful help and version requests. Match [`Self::Parse`] to inspect the
/// original Clap error's kind, exit status, output stream, and rendered text.
///
/// The handler error type is unrestricted. `Display` delegates to the wrapped value, and
/// `Error::source` transparently forwards the wrapped error's source. Match either variant to
/// recover the wrapped error itself.
#[derive(Debug)]
pub enum RunError<E> {
    /// Retains a Clap parser outcome without printing or exiting.
    Parse(clap::Error),
    /// Retains the selected handler's error without requiring a conversion.
    Handler(E),
}

impl<E: fmt::Display> fmt::Display for RunError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(formatter),
            Self::Handler(error) => error.fmt(formatter),
        }
    }
}

impl<E: Error + 'static> Error for RunError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Parse(error) => error.source(),
            Self::Handler(error) => error.source(),
        }
    }
}

// ---------------------------------------------------------------------------------------------- //

struct Dispatcher<R, E> {
    handler: Option<HandlerFn<R, E>>,
    children: Vec<(String, Dispatcher<R, E>)>,
}

impl<R, E> Dispatcher<R, E> {
    fn dispatch(self, matches: &ArgMatches, mut commands: Vec<&Command>) -> Result<R, E> {
        if !self.children.is_empty() {
            if let Some((name, child_matches)) = matches.subcommand() {
                let (_, child) = self
                    .children
                    .into_iter()
                    .find(|(child_name, _)| child_name == name)
                    .expect("parsed subcommand must have a registered handler");
                let command = *commands.last().expect("the command chain must include its root");
                let child_command =
                    command.find_subcommand(name).expect("parsed subcommand must exist");
                commands.push(child_command);
                return child.dispatch(child_matches, commands);
            }
        }
        let handler = self.handler.expect("command without a handler requires a subcommand");
        handler(matches, &commands)
    }
}

type HandlerFn<R, E> = Box<dyn FnOnce(&ArgMatches, &[&Command]) -> Result<R, E>>;

// ---------------------------------------------------------------------------------------------- //

#[cfg(test)]
mod tests;
