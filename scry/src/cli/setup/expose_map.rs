//! Configuration for exposing config values as CLI arguments.

use std::ops::{RangeFrom, RangeInclusive};

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
/// entries can contain one or several assignments. Value-taking entries target one config path.
/// Their tokens remain strings until typed conversion. [`ExposeEntry::array`] groups tokens,
/// while [`ExposeEntry::append`] extends a collection instead of replacing it.
///
/// Use the builder methods ([`option`](Self::option), [`flag`](Self::flag),
/// [`preset`](Self::preset), [`positional`](Self::positional)) or construct directly with public fields.
#[derive(Default, Clone)]
pub struct ExposeMap {
    /// CLI arguments that modify the input config.
    pub entries: Vec<ExposeEntry>,
}

impl ExposeMap {
    /// Creates an empty specification.
    pub fn new() -> Self {
        Self::default()
    }

    /// Exposes a config field as a value-taking CLI option.
    ///
    /// The long name and fallback help are derived from the field path.
    /// An option may occur once unless configured with [`ExposeEntry::append`].
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

    /// Exposes a config field as a positional CLI argument.
    ///
    /// The display name is derived from the field path via SCREAMING_SNAKE_CASE conversion
    /// (for example, `"prefix"` becomes `[PREFIX]`). It is not required by default because the
    /// config file may already provide the value. A final `.array(1..)` positional replaces the
    /// configured array with all its values, including values separated by named options.
    /// Only one positional array is supported. It must be final, cannot coexist with a positional
    /// config path, and may follow only required single-value positionals.
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
    ///
    /// # Panics
    ///
    /// Panics if an entry has an invalid arity, value shape, label, or modifier combination.
    pub fn augment(&self, mut cmd: Command, desc: &Desc) -> Command {
        for entry in &self.entries {
            entry.validate();
            let arg_name = entry.arg_name();
            let mut arg = Arg::new(arg_name.clone());

            match &entry.kind {
                ExposeKind::Positional { .. } => {
                    arg = arg.required(false);
                }
                ExposeKind::Fixed { .. } | ExposeKind::Option { .. } => {
                    if !matches!(entry.long, Long::None) {
                        arg = arg.long(arg_name);
                    }
                    if let Some(short) = entry.short {
                        arg = arg.short(short);
                    }
                }
            }

            if matches!(entry.kind, ExposeKind::Fixed { .. }) {
                arg = arg.action(ArgAction::SetTrue);
            } else {
                arg = entry.configure_values(arg);
            }

            // Whole arrays and variant payloads have their own conversion contracts. Only a
            // scalar replacement can use the target's unit-enum possible-values parser.
            if entry.variant.is_none() && entry.shape == ValueShape::Scalar && !entry.append {
                if let Some(path) = entry.target_path() {
                    if let Some(variants) = enum_variants_for_entry(desc, path) {
                        let possible_values: Vec<PossibleValue> =
                            variants.iter().map(possible_value_from_variant).collect();
                        arg = arg.value_parser(possible_values);
                        arg = arg.ignore_case(true);
                    }
                }
            }

            let mut help_text = entry.help.clone().or_else(|| {
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
            if let Some(suffix) = entry.value_help() {
                match &mut help_text {
                    Some(help) if !help.is_empty() => {
                        help.push(' ');
                        help.push_str(&suffix);
                    }
                    _ => help_text = Some(suffix),
                }
            }
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
            arity: Arity::default(),
            shape: ValueShape::Scalar,
            append: false,
            variant: None,
            help_path,
            long,
            short: None,
            help: None,
            value_name: None,
            value_names: None,
        });
        self.entries.last_mut().unwrap()
    }
}

/// Configures one exposed CLI argument.
#[derive(Clone)]
pub struct ExposeEntry {
    /// Logical CLI name before kebab-case conversion or a custom long-name override.
    pub name: String,
    /// CLI surface and config target for this argument.
    pub kind: ExposeKind,
    /// Number of values consumed by each occurrence.
    pub arity: Arity,
    /// Shape constructed from each occurrence's values.
    pub shape: ValueShape,
    /// Appends values to the configured array and allows repeated named occurrences.
    pub append: bool,
    /// Enum variant key used to wrap one complete assigned value.
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
    /// Display name repeated for each consumed value.
    pub value_name: Option<String>,
    /// Individual display names for an exact number of consumed values.
    pub value_names: Option<Vec<String>>,
}

impl ExposeEntry {
    /// Constructs one array from each occurrence and sets its accepted number of values.
    ///
    /// Accepts an exact count, an inclusive range, or an unbounded range beginning at a positive
    /// count. For example, `.array(2)` constructs a pair and `.array(1..)` constructs a nonempty
    /// array. Combined with [`append`](Self::append), each occurrence appends one complete array.
    /// Otherwise it replaces the target value. A positional array accepts only `1..`.
    ///
    /// This method and [`num_args`](Self::num_args) both set arity. The last call wins.
    ///
    /// # Panics
    ///
    /// Panics for a fixed-assignment entry. Invalid arities are rejected when building a command.
    pub fn array(&mut self, arity: impl Into<Arity>) -> &mut Self {
        self.require_value_taking("array shape");
        self.shape = ValueShape::Array;
        self.arity = arity.into();
        self
    }

    /// Appends this option's values to the configured array and allows repeated occurrences.
    ///
    /// By default each occurrence appends one scalar. [`num_args`](Self::num_args) accepts batches
    /// of scalars, while [`array`](Self::array) groups each occurrence into one appended array.
    /// Appending creates an absent array, but rejects an existing scalar, map, or explicit null.
    /// Defaults supplied during typed conversion are not materialized before appending.
    ///
    /// # Panics
    ///
    /// Panics for fixed assignments, positionals, or entries with a variant wrapper.
    pub fn append(&mut self) -> &mut Self {
        assert!(
            matches!(self.kind, ExposeKind::Option { .. }),
            "exposed {} '{}' cannot append: only named value-taking options support append",
            self.kind.name(),
            self.arg_name()
        );
        assert!(
            self.variant.is_none(),
            "exposed option '{}' cannot combine append with a variant wrapper",
            self.arg_name()
        );
        self.append = true;
        self
    }

    /// Sets the accepted number of values without changing their shape or mutation.
    ///
    /// Scalar replacements require exactly one value. Scalar append options can take positive
    /// batches, such as `.append().num_args(1..)`. This method and [`array`](Self::array) both set
    /// arity. The last call wins.
    /// Variable-length options consume values until the next option, `--`, or their maximum
    /// count. Use `--` to separate a batch from following positional arguments.
    ///
    /// # Panics
    ///
    /// Panics for a fixed-assignment entry. Invalid arities are rejected when building a command.
    pub fn num_args(&mut self, arity: impl Into<Arity>) -> &mut Self {
        self.require_value_taking("arity");
        self.arity = arity.into();
        self
    }

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
    /// The target path names the enum field. `key` is the variant's serialized config key after
    /// any `rename` or `rename_all` rule. The value is wrapped as `#{ key: value }` and assigned
    /// at the target path wholesale.
    ///
    /// Without a custom long name, the CLI name derives from the variant key. Repeated calls
    /// replace the key, with the last call winning.
    ///
    /// # Panics
    ///
    /// Panics for append options, explicit presets, or fixed entries with other than one assignment.
    pub fn variant(&mut self, key: impl Into<String>) -> &mut Self {
        let arg_name = self.arg_name();
        assert!(
            !self.append,
            "exposed option '{arg_name}' cannot combine append with a variant wrapper"
        );
        match &self.kind {
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

    /// Sets one display name for this argument's consumed values.
    ///
    /// Applies to options and positionals, and repeats the label for multiple values. This
    /// replaces any individual labels supplied through [`value_names`](Self::value_names).
    /// Labels never change arity.
    ///
    /// # Panics
    ///
    /// Panics for fixed assignments. Empty labels are rejected when building a command.
    pub fn value_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.require_value_taking("value labels");
        self.value_name = Some(name.into());
        self.value_names = None;
        self
    }

    /// Sets individual display names for an exact number of consumed values.
    ///
    /// The number of labels must match the final arity. This replaces any repeated label supplied
    /// through [`value_name`](Self::value_name). Labels never change arity.
    ///
    /// # Panics
    ///
    /// Panics for fixed assignments. Empty labels, label count mismatches, and labels combined
    /// with variable arity are rejected when building a command.
    pub fn value_names(&mut self, names: impl IntoIterator<Item = impl Into<String>>) -> &mut Self {
        self.require_value_taking("value labels");
        self.value_name = None;
        self.value_names = Some(names.into_iter().map(Into::into).collect());
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
            ExposeKind::Option { path } | ExposeKind::Positional { path } => Some(path),
        }
    }

    fn validate(&self) {
        let arg_name = self.arg_name();
        assert!(
            matches!(self.kind, ExposeKind::Positional { .. })
                || !matches!(self.long, Long::None)
                || self.short.is_some(),
            "exposed {} '{arg_name}' requires a long or short option name. Use positional() for positional input",
            self.kind.name()
        );
        if let ExposeKind::Fixed { assignments } = &self.kind {
            assert!(
                self.arity == Arity::default()
                    && self.shape == ValueShape::Scalar
                    && !self.append
                    && self.value_name.is_none()
                    && self.value_names.is_none(),
                "exposed fixed argument '{arg_name}' cannot configure value arity, shape, append, \
                 or value labels"
            );
            self.validate_fixed(assignments);
            return;
        }

        assert!(
            self.arity.min > 0 && self.arity.max.is_none_or(|max| max >= self.arity.min),
            "exposed {} '{arg_name}' requires a positive, nonempty arity",
            self.kind.name()
        );
        assert!(
            self.shape == ValueShape::Array || self.append || self.arity == Arity::default(),
            "exposed scalar replacement '{arg_name}' requires exactly one value: use array(arity) \
             to construct an array or append() to append scalar batches"
        );
        assert!(
            !self.append || matches!(self.kind, ExposeKind::Option { .. }),
            "exposed {} '{arg_name}' cannot append: only named value-taking options support append",
            self.kind.name()
        );
        assert!(
            !self.append || self.variant.is_none(),
            "exposed option '{arg_name}' cannot combine append with a variant wrapper"
        );

        if matches!(self.kind, ExposeKind::Positional { .. }) {
            assert!(
                matches!(self.long, Long::None) && self.short.is_none(),
                "exposed positional '{arg_name}' cannot have a long or short option name"
            );
            assert!(
                self.shape == ValueShape::Scalar || self.arity == Arity::from(1..),
                "exposed positional array '{arg_name}' requires arity 1.."
            );
        }

        assert!(
            self.value_name.is_none() || self.value_names.is_none(),
            "exposed argument '{arg_name}' cannot specify both value_name and value_names"
        );
        if let Some(name) = &self.value_name {
            assert!(
                !name.trim().is_empty(),
                "exposed argument '{arg_name}' cannot have an empty value label"
            );
        }
        if let Some(names) = &self.value_names {
            assert!(
                self.arity.max == Some(self.arity.min),
                "exposed argument '{arg_name}' requires exact arity for individual value labels"
            );
            assert!(
                names.len() == self.arity.min,
                "exposed argument '{arg_name}' requires {} individual value labels, got {}",
                self.arity.min,
                names.len()
            );
            assert!(
                names.iter().all(|name| !name.trim().is_empty()),
                "exposed argument '{arg_name}' cannot have an empty value label"
            );
        }
    }

    fn validate_fixed(&self, assignments: &[FixedAssignment]) {
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

    fn require_value_taking(&self, modifier: &str) {
        assert!(
            !matches!(self.kind, ExposeKind::Fixed { .. }),
            "exposed fixed argument '{}' cannot configure {modifier}",
            self.arg_name()
        );
    }

    fn configure_values(&self, mut arg: Arg) -> Arg {
        let positional_array =
            matches!(self.kind, ExposeKind::Positional { .. }) && self.shape == ValueShape::Array;
        arg = arg.action(if self.append || positional_array {
            ArgAction::Append
        } else {
            ArgAction::Set
        });
        arg = match self.arity.max {
            Some(max) => arg.num_args(self.arity.min..=max),
            None => arg.num_args(self.arity.min..),
        };

        if let Some(names) = &self.value_names {
            arg.value_names(names.clone())
        } else {
            let name = self.value_name.clone().unwrap_or_else(|| match &self.kind {
                ExposeKind::Positional { path } => path.to_shouty_snake_case(),
                _ if self.append => "ENTRY".to_string(),
                _ => "VALUE".to_string(),
            });
            arg.value_name(name)
        }
    }

    fn value_help(&self) -> Option<String> {
        if matches!(self.kind, ExposeKind::Fixed { .. }) {
            return None;
        }
        let mut help = match (self.shape, self.append) {
            (ValueShape::Array, true) => {
                "Appends one array per occurrence to the configured array. May be repeated."
            }
            (ValueShape::Scalar, true) => {
                "Appends each value to the configured array. May be repeated."
            }
            (ValueShape::Array, false) if matches!(self.kind, ExposeKind::Positional { .. }) => {
                "Replaces the configured array after the final positional value."
            }
            (ValueShape::Array, false) => {
                "Replaces the configured array. May be supplied only once."
            }
            (ValueShape::Scalar, false) => return None,
        }
        .to_string();

        if matches!(self.kind, ExposeKind::Option { .. }) && self.arity.max != Some(self.arity.min)
        {
            help.push_str(
                " Values stop at the next option, '--', or the maximum count. \
                 Use '--' before following positional arguments.",
            );
        }
        Some(help)
    }
}

/// Describes the CLI surface and config target for an exposed argument.
#[derive(Clone, Debug)]
pub enum ExposeKind {
    /// A presence-only argument that applies one or more fixed assignments.
    Fixed { assignments: Vec<FixedAssignment> },
    /// A value-taking option that modifies one config path.
    Option { path: String },
    /// A positional argument that assigns its value to one path.
    Positional { path: String },
}

impl ExposeKind {
    fn name(&self) -> &'static str {
        match self {
            Self::Fixed { .. } => "fixed argument",
            Self::Option { .. } => "option",
            Self::Positional { .. } => "positional",
        }
    }
}

/// Specifies the positive number of values consumed by one occurrence.
///
/// Accepts exact counts (`4`), inclusive ranges (`1..=4`), and unbounded ranges (`1..`). Values
/// constructed through public fields receive the same validation when building a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arity {
    /// Minimum number of values when the argument is present.
    pub min: usize,
    /// Inclusive maximum number of values, or no upper bound.
    pub max: Option<usize>,
}

impl Default for Arity {
    fn default() -> Self {
        Self::from(1)
    }
}

impl From<usize> for Arity {
    fn from(count: usize) -> Self {
        Self {
            min: count,
            max: Some(count),
        }
    }
}

impl From<RangeFrom<usize>> for Arity {
    fn from(range: RangeFrom<usize>) -> Self {
        Self {
            min: range.start,
            max: None,
        }
    }
}

impl From<RangeInclusive<usize>> for Arity {
    fn from(range: RangeInclusive<usize>) -> Self {
        Self {
            min: *range.start(),
            max: Some(*range.end()),
        }
    }
}

/// Determines whether consumed values stay scalar or form one array.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ValueShape {
    /// Keeps each consumed value scalar.
    #[default]
    Scalar,
    /// Groups an occurrence's values into one array, even for exactly one value.
    Array,
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
