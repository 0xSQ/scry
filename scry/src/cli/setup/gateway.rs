//! Flag-only configuration toolbox for Setup commands.

use std::ffi::OsString;

use clap::builder::{OsStringValueParser, PossibleValuesParser};
use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::parser::ValueSource;
use clap::{Arg, ArgAction, ArgMatches, Command};

use super::edits;
use super::{Action, GetFormat, GetRequest, Invocation, SetupError};
use crate::node::Format;

// ---------------------------------------------------------------------------------------------- //

/// Configures the flag-only configuration toolbox entrance.
///
/// The default entrances are `--scry` and `-S`. An application can choose a different long
/// name, replace the short alias, or disable it. Inner edit and action names remain fixed.
#[derive(Clone, Debug)]
pub struct Gateway {
    long: String,
    short: Option<char>,
}

impl Gateway {
    const ID: &'static str = "scry_gateway";

    /// Creates a gateway with the default `--scry` and `-S` entrances.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the long entrance name without the leading `--`.
    ///
    /// # Panics
    ///
    /// Panics if the name is empty, starts with a hyphen, or contains whitespace, control
    /// characters, or `=`.
    pub fn long(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        assert!(
            !name.is_empty()
                && !name.starts_with('-')
                && !name.chars().any(|character| {
                    character.is_whitespace() || character.is_control() || character == '='
                }),
            "gateway long name must be nonempty, omit leading hyphens, and contain no whitespace, control characters, or '='"
        );
        self.long = name;
        self
    }

    /// Sets the short entrance alias, or disables it when given `None`.
    ///
    /// # Panics
    ///
    /// Panics if the alias is a hyphen, whitespace, or a control character.
    pub fn short(mut self, name: impl Into<Option<char>>) -> Self {
        let name = name.into();
        if let Some(name) = name {
            assert!(
                name != '-' && !name.is_whitespace() && !name.is_control(),
                "gateway short alias must not be a hyphen, whitespace, or a control character"
            );
        }
        self.short = name;
        self
    }

    pub(super) fn augment(&self, command: Command) -> Command {
        let mut argument = Arg::new(Self::ID)
            .long(self.long.clone())
            .num_args(0..)
            .value_name("TOOLS")
            .help_heading("Configuration tools")
            .value_parser(OsStringValueParser::new())
            .allow_hyphen_values(true)
            .help("Inspects and adjusts this command's configuration.");
        if let Some(short) = self.short {
            argument = argument.short(short);
        }
        command.arg(argument)
    }

    pub(super) fn parse(
        &self,
        parent_matches: &ArgMatches,
        usage_prefix: &str,
    ) -> Result<Invocation, SetupError> {
        if parent_matches.value_source(Self::ID) != Some(ValueSource::CommandLine) {
            return Ok(Invocation {
                action: Action::Execute,
                edits: Vec::new(),
            });
        }

        let values: Vec<OsString> =
            parent_matches.get_many::<OsString>(Self::ID).into_iter().flatten().cloned().collect();
        let mut command = self.toolbox_command(usage_prefix);
        if values.is_empty() {
            return Ok(Invocation {
                action: Action::Help(command.render_help().to_string()),
                edits: Vec::new(),
            });
        }

        let matches = match command.try_get_matches_from_mut(values) {
            Ok(matches) => matches,
            Err(error) if error.kind() == ErrorKind::DisplayHelp => {
                return Ok(Invocation {
                    action: Action::Help(error.to_string()),
                    edits: Vec::new(),
                });
            }
            Err(mut error) => {
                // Clap omits usage for some missing-value errors. Keep the toolbox scope visible.
                if error.get(ContextKind::Usage).is_none() {
                    error.insert(
                        ContextKind::Usage,
                        ContextValue::StyledStr(command.render_usage()),
                    );
                }
                return Err(error.into());
            }
        };

        let action = match matches.subcommand() {
            Some(("get", request)) => {
                let format = if request.get_flag("flat") {
                    GetFormat::Flat
                } else if let Some(format) = request.get_one::<String>("as") {
                    GetFormat::Format(
                        format.parse::<Format>().expect("registered formats are valid identifiers"),
                    )
                } else {
                    GetFormat::Tree
                };
                Action::Get(GetRequest {
                    path: request.get_one::<String>("key").cloned().unwrap_or_default(),
                    format,
                })
            }
            Some(("desc", request)) => {
                if has_edits(&matches) {
                    return Err(command.error(
                        ErrorKind::ArgumentConflict,
                        "'desc' cannot be combined with '--set' or '--remove' because descriptions do not load or edit configuration",
                    ).into());
                }
                Action::Describe(request.get_one::<String>("key").cloned().unwrap_or_default())
            }
            None => Action::Execute,
            Some(_) => unreachable!("only the fixed toolbox actions are installed"),
        };

        let edits = match action {
            Action::Execute | Action::Get(_) => edits::collect_gateway(&matches)?,
            Action::Describe(_) | Action::Help(_) => Vec::new(),
        };
        Ok(Invocation { action, edits })
    }

    fn toolbox_command(&self, usage_prefix: &str) -> Command {
        let prefix = format!("{usage_prefix} --{}", self.long);
        let example_prefix = match self.short {
            Some(short) => format!("{usage_prefix} -{short}"),
            None => prefix.clone(),
        };
        let command = Command::new("scry")
            .bin_name(prefix.clone())
            .no_binary_name(true)
            .disable_help_subcommand(true)
            .about("Inspects and adjusts this command's configuration.")
            .after_help(
                [
                    format!("Put all application arguments before --{}. Put toolbox edits before get.", self.long),
                    "Edits without a terminal action execute the application command with the edited input.".to_owned(),
                    "Edits affect this invocation and do not write the config file.".to_owned(),
                    "Descriptions cannot be combined with edits.".to_owned(),
                    String::new(),
                    "Examples:".to_owned(),
                    format!("  {example_prefix} get"),
                    format!("  {example_prefix} desc"),
                    format!("  {example_prefix} --set setting value"),
                    format!("  {example_prefix} --set setting value get --as rhai"),
                ]
                .join("\n"),
            );
        let command = command
            .arg(Arg::new("set").long("set").num_args(2).value_names(["KEY", "VALUE"])
                .action(ArgAction::Append).help_heading("Edits")
                .help("Sets a config value to a literal string before execution or get (repeatable)."))
            .arg(Arg::new("remove").long("remove").value_name("KEY")
                .action(ArgAction::Append).help_heading("Edits")
                .help("Removes a config value before execution or get (repeatable)."));

        let formats = crate::node::default_format_writer_registry()
            .supported_format_ids()
            .into_iter()
            .map(|format| format.as_str().to_owned())
            .collect::<Vec<_>>();
        let get = Command::new("get")
            .about("Displays the raw edited configuration without decoding or executing.")
            .arg(
                Arg::new("key")
                    .value_name("KEY")
                    .help("Selects a KeyPath, such as database.host or inputs[0] (omitted = whole input)."),
            )
            .arg(
                Arg::new("flat")
                    .long("flat")
                    .action(ArgAction::SetTrue)
                    .conflicts_with("as")
                    .help("Displays flat key=value lines instead of the default tree view."),
            )
            .arg(
                Arg::new("as")
                    .long("as")
                    .value_name("FORMAT")
                    .value_parser(PossibleValuesParser::new(formats))
                    .help("Serializes the selected input using a registered format writer."),
            )
            .after_help(
                "The output is the loaded Node after application arguments and toolbox edits.\n\
                 Omitted defaults are not filled in. Missing input can be inspected without typed decoding.",
            );
        let desc = Command::new("desc")
            .about("Describes the configuration type without loading or executing.")
            .arg(
                Arg::new("key")
                    .value_name("KEY")
                    .help("Selects a description KeyPath (omitted = full description)."),
            )
            .after_help("Descriptions cannot be combined with --set or --remove.");
        command.subcommand(get).subcommand(desc)
    }
}

impl Default for Gateway {
    fn default() -> Self {
        Self {
            long: "scry".to_owned(),
            short: Some('S'),
        }
    }
}

fn has_edits(matches: &ArgMatches) -> bool {
    ["set", "remove"]
        .into_iter()
        .any(|id| matches.value_source(id) == Some(ValueSource::CommandLine))
}

// ---------------------------------------------------------------------------------------------- //

#[cfg(test)]
mod tests {
    use super::super::edits::Edit;
    use super::*;

    fn parse(gateway: &Gateway, arguments: &[&str]) -> Result<Invocation, SetupError> {
        let matches = gateway
            .augment(Command::new("demo").arg(Arg::new("input")))
            .try_get_matches_from(arguments)?;
        gateway.parse(&matches, "demo operation")
    }

    fn plan(arguments: &[&str]) -> Invocation {
        parse(&Gateway::new(), arguments).unwrap()
    }

    fn error(arguments: &[&str]) -> clap::Error {
        clap_error(parse(&Gateway::new(), arguments).err().expect("invalid toolbox input"))
    }

    fn clap_error(error: SetupError) -> clap::Error {
        match error {
            SetupError::Gateway(error) => error,
            other => panic!("expected a gateway parse error, got {other}"),
        }
    }

    #[test]
    fn absent_gateway_and_word_tokens_are_ordinary_parent_input() {
        for input in ["config", "scry"] {
            let invocation = parse(&Gateway::new(), &["demo", input]).unwrap();
            assert!(matches!(invocation.action, Action::Execute));
            assert!(invocation.edits.is_empty());
        }
        let invocation = parse(&Gateway::new(), &["demo", "--", "--scry"]).unwrap();
        assert!(matches!(invocation.action, Action::Execute));
        assert!(invocation.edits.is_empty());
    }

    #[test]
    fn suffix_escape_and_parent_hyphen_values_keep_clap_semantics() {
        let Action::Get(request) = plan(&["demo", "--scry", "get", "--", "--flat"]).action else {
            panic!("expected get outcome");
        };
        assert_eq!(request.path, "--flat");
        assert!(matches!(request.format, GetFormat::Tree));
        assert_eq!(error(&["demo", "--scry", "--", "get"]).kind(), ErrorKind::UnknownArgument);

        let gateway = Gateway::new();
        let matches = gateway
            .augment(
                Command::new("demo")
                    .arg(Arg::new("literal").long("literal").allow_hyphen_values(true)),
            )
            .try_get_matches_from(["demo", "--literal", "--scry"])
            .unwrap();
        assert_eq!(matches.get_one::<String>("literal").map(String::as_str), Some("--scry"));
        let invocation = gateway.parse(&matches, "demo operation").unwrap();
        assert!(matches!(invocation.action, Action::Execute));
        assert!(invocation.edits.is_empty());
    }

    #[test]
    fn bare_and_explicit_help_use_the_actual_prefix() {
        for arguments in [
            vec!["demo", "--scry"],
            vec!["demo", "--scry", "--help"],
            vec!["demo", "--scry", "--set", "key", "value", "--help"],
        ] {
            let Action::Help(help) = plan(&arguments).action else {
                panic!("help is a successful terminal outcome");
            };
            assert!(help.contains("demo operation --scry"));
            assert!(help.contains("--set <KEY> <VALUE>"));
            assert!(help.contains("Edits without a terminal action execute"));
        }
    }

    #[test]
    fn configured_long_name_and_short_alias_share_the_toolbox() {
        let gateway = Gateway::new().long("look").short('T');
        for entrance in ["--look", "-T"] {
            let plan = parse(&gateway, &["demo", entrance, "get"]).unwrap();
            assert!(matches!(plan.action, Action::Get(_)));
        }
        let Action::Help(help) = parse(&gateway, &["demo", "-T", "get", "--help"]).unwrap().action
        else {
            panic!("action help is a successful terminal outcome");
        };
        assert!(help.contains("demo operation --look get"));
        assert!(parse(&gateway, &["demo", "-S", "get"]).is_err());
    }

    #[test]
    fn invalid_entrance_names_fail_during_configuration() {
        for long in ["", "--look", "look here", "look=here", "look\0"] {
            assert!(std::panic::catch_unwind(|| Gateway::new().long(long)).is_err());
        }
        for short in ['-', ' ', '\0'] {
            assert!(std::panic::catch_unwind(|| Gateway::new().short(short)).is_err());
        }
    }

    #[test]
    fn edits_and_terminal_words_retain_argument_boundaries() {
        let plan = plan(&[
            "demo",
            "input",
            "--scry",
            "--set",
            "name",
            "get",
            "--remove",
            "old",
            "--set",
            "path",
            "two words",
            "get",
            "desc",
        ]);
        let [Edit::Set {
            path: name,
            value: first,
        }, Edit::Remove { path: removed }, Edit::Set {
            path,
            value: second,
        }] = plan.edits.as_slice()
        else {
            panic!("get must retain its preceding edits in argument order");
        };
        assert_eq!(name, "name");
        assert_eq!(first.as_type::<String>().unwrap(), "get");
        assert_eq!(removed, "old");
        assert_eq!(path, "path");
        assert_eq!(second.as_type::<String>().unwrap(), "two words");
        let Action::Get(request) = plan.action else {
            panic!("expected get outcome");
        };
        assert_eq!(request.path, "desc");
        assert!(matches!(request.format, GetFormat::Tree));
    }

    #[test]
    fn edit_only_suffix_selects_normal_execution() {
        let plan = plan(&["demo", "--scry", "--set", "value", "desc"]);
        assert!(matches!(plan.action, Action::Execute));
        let [Edit::Set { path, value }] = plan.edits.as_slice() else {
            panic!("execution must retain its edit");
        };
        assert_eq!(path, "value");
        assert_eq!(value.as_type::<String>().unwrap(), "desc");
    }

    #[test]
    fn rendering_views_and_registered_formats_are_selected() {
        let Action::Get(flat) = plan(&["demo", "--scry", "get", "value", "--flat"]).action else {
            panic!("expected get outcome");
        };
        assert_eq!(flat.path, "value");
        assert!(matches!(flat.format, GetFormat::Flat));
        let Action::Get(rhai) = plan(&["demo", "--scry", "get", "--as", "rhai"]).action else {
            panic!("expected get outcome");
        };
        assert!(matches!(rhai.format, GetFormat::Format(Format::Rhai)));
        assert_eq!(
            error(&["demo", "--scry", "get", "--flat", "--as", "rhai"]).kind(),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            error(&["demo", "--scry", "get", "--as", "unsupported"]).kind(),
            ErrorKind::InvalidValue
        );
    }

    #[test]
    fn descriptions_reject_edits_without_loading() {
        let Action::Describe(path) = plan(&["demo", "--scry", "desc", "field"]).action else {
            panic!("expected description outcome");
        };
        assert_eq!(path, "field");
        let error = error(&["demo", "--scry", "--set", "field", "value", "desc"]);
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
        assert!(error.to_string().contains("demo operation --scry"));
    }

    #[test]
    fn malformed_and_misplaced_suffix_arguments_are_errors() {
        for arguments in [
            vec!["demo", "--scry", "--set", "field"],
            vec!["demo", "--scry", "--remove"],
            vec!["demo", "--scry", "get", "--set", "field", "value"],
            vec!["demo", "--scry", "get", "--scale", "2"],
            vec!["demo", "--scry", "--scry", "get"],
            vec!["demo", "--scry", "unknown"],
        ] {
            let error = error(&arguments);
            assert!(error.to_string().contains("demo operation --scry"), "{arguments:?}: {error}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn non_unicode_suffix_text_is_rejected_without_lossy_conversion() {
        use std::os::windows::ffi::OsStringExt;

        let gateway = Gateway::new();
        let arguments = [
            OsString::from("demo"),
            OsString::from("--scry"),
            OsString::from("get"),
            OsString::from_wide(&[0xd800]),
        ];
        let matches =
            gateway.augment(Command::new("demo")).try_get_matches_from(arguments).unwrap();
        let error = clap_error(gateway.parse(&matches, "demo operation").err().unwrap());
        assert_eq!(error.kind(), ErrorKind::InvalidUtf8);
    }
}
