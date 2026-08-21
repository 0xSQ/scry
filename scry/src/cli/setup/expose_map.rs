//! Configuration for exposing config values as CLI arguments.

use clap::builder::PossibleValue;
use clap::{Arg, ArgAction, Command};

use heck::{ToKebabCase, ToShoutySnakeCase};

use crate::desc::{EntryRef, VariantDesc, VariantRepr};
use crate::node::Node;
use crate::{Desc, ToNode};

// ---------------------------------------------------------------------------------------------- //

/// Specifies which config values to expose as CLI arguments.
///
/// Each entry describes one CLI argument and the config operation it performs. Presence-only fixed
/// entries can contain one or several assignments; value-taking entries target one config path.
///
/// Use the builder methods ([`option`](Self::option), [`flag`](Self::flag),
/// [`preset`](Self::preset), [`list`](Self::list)) or construct directly with public fields.
#[derive(Default, Clone)]
pub struct ExposeMap {
    /// CLI arguments that modify the input config.
    pub entries: Vec<ExposeEntry>,
}

/// Configures one exposed CLI argument.
#[derive(Clone)]
pub struct ExposeEntry {
    /// Logical CLI name before kebab-case conversion or a custom long-name override.
    pub name: String,
    /// Config operation performed by this argument.
    pub kind: ExposeKind,
    /// Enum variant key used to wrap a single assigned value.
    ///
    /// `None` assigns the value at its path directly.
    pub variant: Option<String>,
    /// Config path whose documentation supplies fallback help.
    ///
    /// Presets have no primary config path and therefore leave this unset.
    pub help_path: Option<String>,
    /// Long option behavior.
    pub long: Long,
    /// Short option character.
    pub short: Option<char>,
    /// Custom help text.
    pub help: Option<String>,
    /// Custom display name for positional arguments.
    pub value_name: Option<String>,
}

/// Describes the config operation performed by an exposed CLI argument.
#[derive(Clone, Debug)]
pub enum ExposeKind {
    /// A presence-only argument that applies one or more fixed assignments.
    Fixed { assignments: Vec<FixedAssignment> },
    /// A value-taking option that assigns its argument to one path.
    Option { path: String },
    /// A repeatable option that appends each argument to one list path.
    List { path: String },
    /// A positional argument that assigns its value to one path.
    Positional { path: String },
}

/// One fixed config assignment carried by a flag or preset.
#[derive(Clone, Debug)]
pub struct FixedAssignment {
    /// Config path to assign.
    pub path: String,
    /// Preconfigured value assigned when the argument is present.
    pub value: Node,
}

/// Controls the long option name for an exposed argument.
#[derive(Clone, Debug)]
pub enum Long {
    /// No long option (short flag only).
    None,
    /// Derives the long name from the variant key when set, otherwise from the logical name.
    Auto,
    /// Uses a custom long option name.
    Custom(String),
}

// ---------------------------------------------------------------------------------------------- //

impl ExposeMap {
    /// Creates an empty specification.
    pub fn new() -> Self {
        Self::default()
    }

    /// Exposes a config field as a value-taking CLI option.
    ///
    /// The long name and fallback help are derived from the field path.
    pub fn option(&mut self, path: impl Into<String>) -> &mut ExposeEntry {
        let path = path.into();
        self.push_entry(
            path.clone(),
            ExposeKind::Option { path: path.clone() },
            Some(path),
            Long::Auto,
        )
    }

    /// Exposes one fixed assignment as a presence-only CLI flag.
    ///
    /// This is the concise form of a one-assignment [`preset`](Self::preset): the config path also
    /// supplies the default long name and fallback help. The flag leaves the loaded config
    /// unchanged when absent. Any value supported by [`ToNode`] can be assigned.
    ///
    /// # Panics
    ///
    /// Panics if the fixed value cannot be converted to a [`Node`].
    pub fn flag(&mut self, path: impl Into<String>, value: impl ToNode) -> &mut ExposeEntry {
        let path = path.into();
        self.push_entry(
            path.clone(),
            ExposeKind::Fixed {
                assignments: Vec::new(),
            },
            Some(path.clone()),
            Long::Auto,
        )
        .set(path, value)
    }

    /// Exposes a named presence-only CLI preset containing fixed assignments.
    ///
    /// Add assignments with [`ExposeEntry::set`]. A preset shares the same fixed-assignment
    /// mechanism as [`flag`](Self::flag), but names the CLI concept independently from its config
    /// paths and does not inherit help from any one assignment.
    ///
    /// # Panics
    ///
    /// Building a command panics if the preset contains no assignments.
    pub fn preset(&mut self, name: impl Into<String>) -> &mut ExposeEntry {
        self.push_entry(
            name.into(),
            ExposeKind::Fixed {
                assignments: Vec::new(),
            },
            None,
            Long::Auto,
        )
    }

    /// Exposes a config field as a repeatable list CLI option.
    ///
    /// Each occurrence appends one element to the config array. For example,
    /// `--items a --items b` produces `["a", "b"]`. The long name and fallback help are derived
    /// from the field path.
    pub fn list(&mut self, path: impl Into<String>) -> &mut ExposeEntry {
        let path = path.into();
        self.push_entry(
            path.clone(),
            ExposeKind::List { path: path.clone() },
            Some(path),
            Long::Auto,
        )
    }

    /// Exposes a config field as a positional CLI argument.
    ///
    /// The display name is derived from the field path via SCREAMING_SNAKE_CASE conversion
    /// (for example, `"prefix"` becomes `[PREFIX]`). It is not required by default because the
    /// config file may already provide the value.
    pub fn positional(&mut self, path: impl Into<String>) -> &mut ExposeEntry {
        let path = path.into();
        self.push_entry(
            path.clone(),
            ExposeKind::Positional { path: path.clone() },
            Some(path),
            Long::None,
        )
    }

    /// Adds the exposed entries as arguments to a Clap command.
    pub fn augment(&self, mut cmd: Command, desc: &Desc) -> Command {
        for entry in &self.entries {
            entry.validate();
            let arg_name = entry.arg_name();
            let mut arg = Arg::new(arg_name.clone());

            match &entry.kind {
                ExposeKind::Positional { path } => {
                    let display =
                        entry.value_name.clone().unwrap_or_else(|| path.to_shouty_snake_case());
                    arg = arg.value_name(display).required(false);
                }
                kind => {
                    if !matches!(entry.long, Long::None) {
                        arg = arg.long(arg_name);
                    }
                    match kind {
                        ExposeKind::Fixed { .. } => {
                            arg = arg.action(ArgAction::SetTrue);
                        }
                        ExposeKind::Option { .. } => {
                            arg = arg.num_args(1).value_name("VALUE");
                        }
                        ExposeKind::List { .. } => {
                            arg = arg.num_args(1).value_name("ENTRY").action(ArgAction::Append);
                        }
                        ExposeKind::Positional { .. } => unreachable!(),
                    }
                    if let Some(short) = entry.short {
                        arg = arg.short(short);
                    }
                }
            }

            // Variant entries carry the arm's payload as their CLI value, not an enum name, so
            // the unit-enum possible-values parser must not apply to them.
            if entry.variant.is_none() && !matches!(&entry.kind, ExposeKind::Fixed { .. }) {
                if let Some(path) = entry.target_path() {
                    if let Some(variants) = enum_variants_for_entry(desc, path) {
                        let possible_values: Vec<PossibleValue> =
                            variants.iter().map(possible_value_from_variant).collect();
                        arg = arg.value_parser(possible_values);
                        arg = arg.ignore_case(true);
                    }
                }
            }

            let help_text = entry.help.clone().or_else(|| {
                entry.help_path.as_deref().and_then(|path| {
                    desc.entry_at_path(path).and_then(|entry| {
                        let doc = entry.doc();
                        if doc.is_empty() {
                            None
                        } else {
                            Some(doc.to_string())
                        }
                    })
                })
            });
            if let Some(help) = help_text {
                arg = arg.help(help);
            }

            cmd = cmd.arg(arg);
        }

        cmd
    }

    fn push_entry(
        &mut self,
        name: String,
        kind: ExposeKind,
        help_path: Option<String>,
        long: Long,
    ) -> &mut ExposeEntry {
        self.entries.push(ExposeEntry {
            name,
            kind,
            variant: None,
            help_path,
            long,
            short: None,
            help: None,
            value_name: None,
        });
        self.entries.last_mut().unwrap()
    }
}

fn enum_variants_for_entry<'a>(desc: &'a Desc, path: &str) -> Option<&'a [VariantDesc]> {
    match desc.entry_at_path(path)? {
        EntryRef::Field(field) => field.value.unit_enum_variants(),
        EntryRef::TupleElem { value, .. } => value.unit_enum_variants(),
        EntryRef::Variant(_) => None,
    }
}

fn possible_value_from_variant(variant: &VariantDesc) -> PossibleValue {
    let (display_name, aliases) = cli_variant_value_names(&variant.name);
    let mut possible = PossibleValue::new(display_name);

    if !aliases.is_empty() {
        possible = possible.aliases(aliases);
    }

    if let Some(help) = variant_help_text(variant) {
        possible = possible.help(help);
    }

    possible
}

fn cli_variant_value_names(name: &str) -> (String, Vec<String>) {
    if name.contains('_') {
        (name.replace('_', "-"), vec![name.to_string()])
    } else if name.contains('-') {
        (name.to_string(), vec![name.replace('-', "_")])
    } else {
        (name.to_string(), Vec::new())
    }
}

fn variant_help_text(variant: &VariantDesc) -> Option<String> {
    let doc = variant.doc.lines().find(|line| !line.trim().is_empty()).map(str::trim);
    let is_default = matches!(variant.repr, VariantRepr::Unit { is_default: true });

    match (doc, is_default) {
        (Some(doc), true) => Some(format!("{doc} [default]")),
        (Some(doc), false) => Some(doc.to_string()),
        (None, true) => Some("[default]".to_string()),
        (None, false) => None,
    }
}

impl ExposeEntry {
    /// Adds a fixed config assignment to this flag or preset.
    ///
    /// # Panics
    ///
    /// Panics if this is not a fixed-assignment entry, if the path is already assigned by this
    /// entry, or if the value cannot be converted to a [`Node`].
    pub fn set(&mut self, path: impl Into<String>, value: impl ToNode) -> &mut Self {
        let path = path.into();
        let arg_name = self.arg_name();
        let kind_name = self.kind.name();
        let ExposeKind::Fixed { assignments } = &mut self.kind else {
            panic!("exposed {kind_name} '{arg_name}' cannot add fixed assignment '{path}'");
        };

        assert!(
            !assignments.iter().any(|assignment| assignment.path == path),
            "exposed fixed argument '{arg_name}' already assigns config path '{path}'"
        );

        let value = value.to_node().unwrap_or_else(|error| {
            panic!(
                "failed to configure assignment '{path}' for exposed fixed argument \
                 '{arg_name}': {error}"
            )
        });
        assignments.push(FixedAssignment { path, value });
        self
    }

    /// Wraps this entry's single assigned value in one enum variant.
    ///
    /// The target path names the enum field; `key` is the variant's serialized config key after
    /// any `rename` or `rename_all` rule. The value is wrapped as `#{ key: value }` and assigned
    /// at the target path wholesale.
    ///
    /// Without a custom long name, the CLI name derives from the variant key. Repeated calls
    /// replace the key, with the last call winning.
    ///
    /// # Panics
    ///
    /// Panics for lists, explicit presets, or fixed entries with other than one assignment.
    pub fn variant(&mut self, key: impl Into<String>) -> &mut Self {
        let arg_name = self.arg_name();
        match &self.kind {
            ExposeKind::List { path } => {
                panic!(
                    "exposed list '{path}' cannot take a variant wrapper: \
                     append-inside-a-variant semantics are not defined"
                );
            }
            ExposeKind::Fixed { assignments } => {
                assert!(
                    self.help_path.is_some() && assignments.len() == 1,
                    "exposed fixed argument '{arg_name}' cannot take a variant wrapper unless it \
                     is a one-assignment field flag"
                );
            }
            ExposeKind::Option { .. } | ExposeKind::Positional { .. } => {}
        }

        self.variant = Some(key.into());
        self
    }

    /// Sets a custom long option name.
    pub fn long(&mut self, name: impl Into<String>) -> &mut Self {
        self.long = Long::Custom(name.into());
        self
    }

    /// Suppresses the long option, leaving only the short flag.
    pub fn no_long(&mut self) -> &mut Self {
        self.long = Long::None;
        self
    }

    /// Sets the short option character.
    pub fn short(&mut self, c: char) -> &mut Self {
        self.short = Some(c);
        self
    }

    /// Sets custom help text, overriding config-derived fallback help.
    pub fn help(&mut self, text: impl Into<String>) -> &mut Self {
        self.help = Some(text.into());
        self
    }

    /// Sets the display name for positional arguments.
    ///
    /// Only meaningful for [`ExposeKind::Positional`]. For example,
    /// `.value_name("PATTERN")` displays as `[PATTERN]`.
    pub fn value_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.value_name = Some(name.into());
        self
    }

    /// Returns the argument name used as the Clap argument ID.
    ///
    /// [`Long::Auto`] and [`Long::None`] derive from the variant key when present, otherwise from
    /// the logical name. [`Long::Custom`] returns its configured name unchanged.
    pub fn arg_name(&self) -> String {
        match &self.long {
            Long::Auto | Long::None => self.variant.as_ref().unwrap_or(&self.name).to_kebab_case(),
            Long::Custom(name) => name.clone(),
        }
    }

    pub(crate) fn target_path(&self) -> Option<&str> {
        match &self.kind {
            ExposeKind::Fixed { .. } => None,
            ExposeKind::Option { path }
            | ExposeKind::List { path }
            | ExposeKind::Positional { path } => Some(path),
        }
    }

    fn validate(&self) {
        let ExposeKind::Fixed { assignments } = &self.kind else {
            return;
        };

        assert!(
            !assignments.is_empty(),
            "exposed preset '{}' must contain at least one fixed assignment",
            self.arg_name()
        );

        assert!(
            self.variant.is_none() || (self.help_path.is_some() && assignments.len() == 1),
            "exposed fixed argument '{}' cannot take a variant wrapper unless it is a \
             one-assignment field flag",
            self.arg_name()
        );

        for (index, assignment) in assignments.iter().enumerate() {
            assert!(
                !assignments[..index].iter().any(|prior| prior.path == assignment.path),
                "exposed fixed argument '{}' assigns config path '{}' more than once",
                self.arg_name(),
                assignment.path
            );
        }
    }
}

impl ExposeKind {
    fn name(&self) -> &'static str {
        match self {
            Self::Fixed { .. } => "fixed argument",
            Self::Option { .. } => "option",
            Self::List { .. } => "list",
            Self::Positional { .. } => "positional",
        }
    }
}
