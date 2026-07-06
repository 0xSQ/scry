//! Configuration for exposing config fields as CLI arguments.

use clap::builder::PossibleValue;
use clap::{Arg, ArgAction, Command};

use heck::{ToKebabCase, ToShoutySnakeCase};

use crate::desc::{EntryRef, VariantDesc, VariantRepr};
use crate::node::Node;
use crate::{Desc, ToNode};

// ---------------------------------------------------------------------------------------------- //

/// Specifies which config fields to expose as CLI arguments.
///
/// Each field in the config can be exposed as a CLI argument, allowing users
/// to override config values directly from the command line.
///
/// Use the builder methods ([`option`](Self::option), [`flag`](Self::flag),
/// [`list`](Self::list)) or construct directly with public fields.
#[derive(Default, Clone)]
pub struct ExposeMap {
    /// List of config fields to expose as CLI arguments.
    pub entries: Vec<ExposeEntry>,
}

/// Configuration for a single exposed field.
#[derive(Clone)]
pub struct ExposeEntry {
    /// Config path (e.g., "format" or "host.port").
    pub path: String,
    /// The kind of CLI argument this field is exposed as.
    pub kind: ExposeKind,
    /// Enum variant key to wrap the value in (`#{ key: value }`) before assignment.
    /// `None` assigns the value at the path directly.
    pub variant: Option<String>,
    /// Long option behavior. Defaults to deriving from the variant key when set, else from the
    /// path (kebab-case).
    pub long: Long,
    /// Short option character.
    pub short: Option<char>,
    /// Custom help text. If None, the field's config description is used.
    pub help: Option<String>,
    /// Custom display name for positional arguments (e.g., "PATTERN").
    /// If None, derived from path via SCREAMING_SNAKE_CASE. Only used for positionals.
    pub value_name: Option<String>,
}

/// The kind of CLI argument an exposed config field maps to.
#[derive(Clone, Debug)]
pub enum ExposeKind {
    /// A presence-only flag that assigns a fixed value when present.
    Flag { value: Node },
    /// A value-taking option (e.g., `--port 8080`).
    Option,
    /// A repeatable list option where each occurrence appends one element.
    List,
    /// A positional argument (e.g., `[PREFIX]`).
    Positional,
}

/// Controls the long option name for an exposed config field.
#[derive(Clone, Debug)]
pub enum Long {
    /// No long option (short flag only).
    None,
    /// Derives the long name via kebab-case conversion, from the variant key when the entry has
    /// one, else from the config path.
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
    /// The long name is derived from the field path (snake_case to kebab-case).
    pub fn option(&mut self, path: impl Into<String>) -> &mut ExposeEntry {
        self.entries.push(ExposeEntry {
            path: path.into(),
            kind: ExposeKind::Option,
            variant: None,
            long: Long::Auto,
            short: None,
            help: None,
            value_name: None,
        });
        self.entries.last_mut().unwrap()
    }

    /// Exposes a fixed assignment as a presence-only CLI flag.
    ///
    /// The flag is syntactic sugar for a fixed `--set PATH VALUE` operation: it assigns `value`
    /// when the flag is present and leaves the loaded config unchanged when absent. The value is
    /// usually boolean `true`, but any [`ToNode`] value works. The long name is derived from the
    /// field path (snake_case to kebab-case).
    ///
    /// # Panics
    ///
    /// Panics if the fixed value cannot be converted to a [`Node`].
    pub fn flag(&mut self, path: impl Into<String>, value: impl ToNode) -> &mut ExposeEntry {
        let path = path.into();
        let value = value.to_node().unwrap_or_else(|error| {
            panic!("failed to configure value for exposed flag '{path}': {error}")
        });
        self.entries.push(ExposeEntry {
            path,
            kind: ExposeKind::Flag { value },
            variant: None,
            long: Long::Auto,
            short: None,
            help: None,
            value_name: None,
        });
        self.entries.last_mut().unwrap()
    }

    /// Exposes a config field as a repeatable list CLI option.
    ///
    /// Each occurrence appends one element to the config array. For example,
    /// `--items a --items b` produces `["a", "b"]`. The long name is derived
    /// from the field path (snake_case to kebab-case).
    pub fn list(&mut self, path: impl Into<String>) -> &mut ExposeEntry {
        self.entries.push(ExposeEntry {
            path: path.into(),
            kind: ExposeKind::List,
            variant: None,
            long: Long::Auto,
            short: None,
            help: None,
            value_name: None,
        });
        self.entries.last_mut().unwrap()
    }

    /// Exposes a config field as a positional CLI argument.
    ///
    /// The display name is derived from the field path via SCREAMING_SNAKE_CASE
    /// conversion (e.g., `"prefix"` becomes `[PREFIX]`). Not required by default,
    /// since the config file may provide the value.
    pub fn positional(&mut self, path: impl Into<String>) -> &mut ExposeEntry {
        self.entries.push(ExposeEntry {
            path: path.into(),
            kind: ExposeKind::Positional,
            variant: None,
            long: Long::None,
            short: None,
            help: None,
            value_name: None,
        });
        self.entries.last_mut().unwrap()
    }

    /// Adds the exposed entries as arguments to a clap Command.
    pub fn augment(&self, mut cmd: Command, desc: &Desc) -> Command {
        for entry in &self.entries {
            let arg_name = entry.arg_name();
            let mut arg = Arg::new(arg_name.clone());

            // Configure the argument based on its kind.
            match &entry.kind {
                ExposeKind::Positional => {
                    let display = entry
                        .value_name
                        .clone()
                        .unwrap_or_else(|| entry.path.to_shouty_snake_case());
                    arg = arg.value_name(display).required(false);
                }
                kind => {
                    if !matches!(entry.long, Long::None) {
                        arg = arg.long(arg_name);
                    }
                    match kind {
                        ExposeKind::Flag { .. } => {
                            arg = arg.action(ArgAction::SetTrue);
                        }
                        ExposeKind::Option => {
                            arg = arg.num_args(1).value_name("VALUE");
                        }
                        ExposeKind::List => {
                            arg = arg.num_args(1).value_name("ENTRY").action(ArgAction::Append);
                        }
                        ExposeKind::Positional => unreachable!(),
                    }
                    if let Some(short) = entry.short {
                        arg = arg.short(short);
                    }
                }
            }

            // Variant entries carry the arm's payload as their CLI value, not an enum name, so
            // the unit-enum possible-values parser must not apply to them.
            if entry.variant.is_none() && !matches!(&entry.kind, ExposeKind::Flag { .. }) {
                if let Some(variants) = enum_variants_for_entry(desc, &entry.path) {
                    let possible_values: Vec<PossibleValue> =
                        variants.iter().map(possible_value_from_variant).collect();
                    arg = arg.value_parser(possible_values);
                    arg = arg.ignore_case(true);
                }
            }

            // Use explicit help text, or fall back to config description.
            let help_text = entry.help.clone().or_else(|| {
                desc.entry_at_path(&entry.path).and_then(|entry| {
                    let doc = entry.doc();
                    if doc.is_empty() {
                        None
                    } else {
                        Some(doc.to_string())
                    }
                })
            });
            if let Some(help) = help_text {
                arg = arg.help(help);
            }

            cmd = cmd.arg(arg);
        }

        cmd
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
    /// Wraps this entry's value in a single-key map, addressing one variant of an enum field.
    ///
    /// The entry's path names the enum field; `key` is the variant's *serialized* config key
    /// (after any `rename`/`rename_all`), not necessarily the Rust variant name. At apply time
    /// the computed value is wrapped as `#{ key: value }` and assigned at the path wholesale, so
    /// selecting one arm always displaces whichever arm the config held. This reaches arms whose
    /// payload parses from a single CLI string (or, for flags, from the fixed node); unit
    /// variants of derived enums serialize as bare strings and are better served by a plain
    /// fixed-value flag. Repeated calls replace the key (the last call wins); nesting is not
    /// supported.
    ///
    /// Without a custom long name, the long option name derives from the variant key rather
    /// than the path, so `e.option("source").variant("grid")` exposes `--grid`.
    ///
    /// # Panics
    ///
    /// Panics for list entries; append-inside-a-variant semantics are not defined.
    pub fn variant(&mut self, key: impl Into<String>) -> &mut Self {
        assert!(
            !matches!(self.kind, ExposeKind::List),
            "exposed list '{}' cannot take a variant wrapper: \
             append-inside-a-variant semantics are not defined",
            self.path
        );
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

    /// Sets custom help text, overriding the config description.
    pub fn help(&mut self, text: impl Into<String>) -> &mut Self {
        self.help = Some(text.into());
        self
    }

    /// Sets the display name for positional arguments.
    ///
    /// Only meaningful for [`ExposeKind::Positional`]. For example,
    /// `.value_name("PATTERN")` displays as `[PATTERN]` in help text.
    pub fn value_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.value_name = Some(name.into());
        self
    }

    /// Returns the argument name used as the clap argument ID.
    ///
    /// For [`Long::Auto`] and [`Long::None`], the variant key (when set) or otherwise the path
    /// is converted to kebab-case. For [`Long::Custom`], the custom name is used.
    pub fn arg_name(&self) -> String {
        match &self.long {
            Long::Auto | Long::None => match &self.variant {
                Some(key) => key.to_kebab_case(),
                None => self.path.to_kebab_case(),
            },
            Long::Custom(name) => name.clone(),
        }
    }
}
