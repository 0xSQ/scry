//! Provides [`Bundle`] for combining clap commands with dispatch logic.

use clap::{ArgMatches, Command};

// ---------------------------------------------------------------------------------------------- //

/// A clap command paired with its dispatch logic.
///
/// The type parameters are:
/// - `R` - what the handler function returns (typically a Result type, or `()`)
/// - `E` - the error type
///
/// When `run()` is called, it returns `Result<R, E>`.
pub struct Bundle<R, E> {
    pub(crate) command: Command,
    pub(crate) dispatcher: Dispatcher<R, E>,
}

/// Dispatches argument matches to the appropriate handler.
pub struct Dispatcher<R, E> {
    name: String,
    kind: Kind<R, E>,
}

enum Kind<R, E> {
    Single(HandlerFn<R, E>),
    Group(Group<R, E>),
}

struct Group<R, E> {
    children: Vec<Dispatcher<R, E>>,
    fallback: Option<HandlerFn<R, E>>,
}

type HandlerFn<R, E> = Box<dyn FnMut(&ArgMatches) -> Result<R, E>>;

// ---------------------------------------------------------------------------------------------- //
// Bundle<R, E>

impl<R: 'static, E: 'static> Bundle<R, E> {
    /// Creates a bundle for a single command with the given handler.
    ///
    /// The handler receives `&ArgMatches` and returns `Result<R, E>`.
    pub fn single<F>(command: Command, handler: F) -> Self
    where
        F: FnOnce(&ArgMatches) -> Result<R, E> + 'static,
    {
        let name = command.get_name().to_string();
        let mut handler = Some(handler);
        Bundle {
            command,
            dispatcher: Dispatcher {
                name,
                kind: Kind::Single(Box::new(move |matches| {
                    let f = handler.take().expect("handler called more than once");
                    f(matches)
                })),
            },
        }
    }

    /// Creates a bundle that groups other bundles as subcommands.
    ///
    /// By default, a subcommand is required. Use [`fallback`](Self::fallback)
    /// to allow running without a subcommand.
    pub fn group(
        name: impl Into<String>,
        about: impl Into<String>,
        bundles: Vec<Bundle<R, E>>,
    ) -> Self {
        Self::group_with(Command::new(name.into()).about(about.into()), bundles)
    }

    /// Creates a bundle that groups other bundles as subcommands, with full control
    /// over the parent command.
    ///
    /// Use this when you need to configure additional arguments, styles, or other
    /// settings on the parent command.
    pub fn group_with(command: Command, bundles: Vec<Bundle<R, E>>) -> Self {
        let name = command.get_name().to_string();
        let mut command = command.subcommand_required(true);

        let mut children = Vec::new();
        for bundle in bundles {
            command = command.subcommand(bundle.command);
            children.push(bundle.dispatcher);
        }

        Bundle {
            command,
            dispatcher: Dispatcher {
                name,
                kind: Kind::Group(Group {
                    children,
                    fallback: None,
                }),
            },
        }
    }

    /// Adds a fallback handler for when no subcommand is given.
    ///
    /// This makes the subcommand optional - the fallback runs when the user
    /// runs the parent command without specifying a subcommand.
    pub fn fallback<F>(mut self, fallback: F) -> Self
    where
        F: FnOnce(&ArgMatches) -> Result<R, E> + 'static,
    {
        // Make subcommand optional
        self.command = self.command.subcommand_required(false);

        // Set the fallback on the dispatcher
        let mut fallback = Some(fallback);
        self.dispatcher.set_fallback(Box::new(move |matches| {
            let f = fallback.take().expect("fallback called more than once");
            f(matches)
        }));

        self
    }

    /// Runs the CLI by parsing arguments and dispatching to the handler.
    pub fn run(self) -> Result<R, E> {
        let Bundle {
            command,
            mut dispatcher,
        } = self;
        let matches = command.get_matches();
        dispatcher.dispatch(&matches)
    }

    /// Runs with custom arguments instead of reading from the command line.
    ///
    /// Useful for testing.
    pub fn run_from<I, S>(self, args: I) -> Result<R, E>
    where
        I: IntoIterator<Item = S>,
        S: Into<std::ffi::OsString> + Clone,
    {
        let Bundle {
            command,
            mut dispatcher,
        } = self;
        let matches = command.get_matches_from(args);
        dispatcher.dispatch(&matches)
    }

    /// Returns a reference to the underlying clap Command.
    ///
    /// Useful for introspection in tests.
    pub fn command(&self) -> &Command {
        &self.command
    }
}

// ---------------------------------------------------------------------------------------------- //
// Dispatcher<R, E>

impl<R: 'static, E: 'static> Dispatcher<R, E> {
    /// Creates a dispatcher with the given handler function.
    pub fn new(
        name: impl Into<String>,
        handler: impl FnMut(&ArgMatches) -> Result<R, E> + 'static,
    ) -> Self {
        Dispatcher {
            name: name.into(),
            kind: Kind::Single(Box::new(handler)),
        }
    }

    /// Sets the fallback handler for a group dispatcher.
    ///
    /// # Panics
    ///
    /// Panics if this dispatcher is not a group.
    fn set_fallback(&mut self, fallback: HandlerFn<R, E>) {
        match &mut self.kind {
            Kind::Group(group) => group.fallback = Some(fallback),
            Kind::Single(_) => panic!("cannot set fallback on a single dispatcher"),
        }
    }

    /// Dispatches to the appropriate handler based on the argument matches.
    pub fn dispatch(&mut self, matches: &ArgMatches) -> Result<R, E> {
        match &mut self.kind {
            Kind::Single(handler) => handler(matches),
            Kind::Group(group) => {
                if let Some((sub_name, sub_matches)) = matches.subcommand() {
                    for child in &mut group.children {
                        if child.name == sub_name {
                            return child.dispatch(sub_matches);
                        }
                    }
                    unreachable!("unknown subcommand: {}", sub_name);
                }
                match &mut group.fallback {
                    Some(fallback_fn) => fallback_fn(matches),
                    None => unreachable!("no subcommand provided and no fallback"),
                }
            }
        }
    }
}
