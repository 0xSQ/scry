//! Exercises independent environment file and binding conventions through public APIs.

use std::cell::Cell;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use scry::clap::Command;
use scry::cli::setup::{ApplicationConfig, CommandConfigError, LoadedCommandConfig, Setup};
use scry::node::{default_format_parser_registry_builder, ConfigFormatParser, Format};
use scry::util::PathExt;
use scry::{Config, KeyPath, Node, NodeError};
use tempfile::tempdir;

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config)]
struct NumberConfig {
    /// Supplies the command's value.
    value: i64,
}

#[test]
fn defaults_and_each_independent_override_load_the_expected_binding() {
    for (file, binding) in [
        ("env", "env"),
        ("machine", "env"),
        ("env", "host"),
        ("machine", "host"),
    ] {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join(format!("{file}.rhai")), "#{ value: 17 }").unwrap();
        let mut application = ApplicationConfig::new("atlas").directory(directory.path());
        if file != "env" {
            application = application.environment_file(file);
        }
        if binding != "env" {
            application = application.environment_binding(binding);
        }
        let environment = application.load_environment().unwrap();
        let scope = environment.scope();
        let value = scope.get_value::<rhai::Map>(binding).unwrap();
        assert_eq!(value["value"].clone().cast::<i64>(), 17);
        if binding != "env" {
            assert!(!scope.contains("env"));
        }
    }
}

#[test]
fn returned_environment_value_preserves_functions_and_ignores_scope_binding_names() {
    let directory = tempdir().unwrap();
    fs::write(
        directory.path().join("machine.rhai"),
        indoc::indoc! {r#"
            fn callback() { 29 }
            export let env = #{ value: "wrong env" };
            export let host = #{ value: "wrong host" };
            #{ value: "returned", callback: Fn("callback") }
        "#},
    )
    .unwrap();
    let application = custom_environment(directory.path());
    let environment = application.load_environment().unwrap();
    let value = environment.scope().get_value::<rhai::Map>("host").unwrap();
    assert_eq!(value["value"].clone().cast::<String>(), "returned");
    assert!(value["callback"].is::<rhai::FnPtr>());
}

#[test]
fn custom_binding_reaches_the_command_and_its_imported_modules() {
    let directory = tempdir().unwrap();
    fs::create_dir(directory.path().join("lib")).unwrap();
    fs::write(directory.path().join("machine.rhai"), "#{ value: \"machine\" }").unwrap();
    fs::write(directory.path().join("lib/base.rhai"), "export let value = host.value;").unwrap();
    fs::write(
        directory.path().join("shared.rhai"),
        "import \"lib/base.rhai\" as base; export let value = base::value;",
    )
    .unwrap();
    fs::write(
        directory.path().join("render.rhai"),
        "import \"shared.rhai\" as values; #{ value: host.value + \":\" + values::value + \":\" + app_value() }",
    )
    .unwrap();
    let application = custom_environment(directory.path()).rhai_engine(|engine, environment| {
        let value = environment
            .scope()
            .get_value::<rhai::Map>("host")
            .and_then(|map| map.get("value").cloned())
            .and_then(|value| value.try_cast::<String>())
            .unwrap_or_else(|| "not loaded".into());
        engine.register_fn("app_value", move || format!("callback-{value}"));
    });
    let loaded = invocation("render").load(&application).unwrap().unwrap();
    assert_eq!(loaded.node.req::<String>("value").unwrap(), "machine:machine:callback-machine");
}

#[test]
fn only_the_configured_file_stem_is_reserved_for_environment_loading() {
    let directory = tempdir().unwrap();
    fs::create_dir(directory.path().join("machine")).unwrap();
    fs::write(directory.path().join("machine.rhai"), "#{ value: \"environment\" }").unwrap();
    fs::write(directory.path().join("env.rhai"), "#{ value: \"env command\" }").unwrap();
    fs::write(directory.path().join("host.rhai"), "#{ value: \"host command\" }").unwrap();
    fs::write(directory.path().join("machine/show.rhai"), "#{ value: host.value }").unwrap();
    fs::write(
        directory.path().join("atlas.rhai"),
        "#{ machine: #{ \"$self\": #{ value: \"app-wide machine command\" } } }",
    )
    .unwrap();
    let root = Command::new("atlas").subcommand_required(true).subcommands([
        Command::new("env"),
        Command::new("host"),
        Command::new("machine").subcommand(Command::new("show")),
    ]);
    let application = custom_environment(directory.path());
    for (path, expected) in [
        (vec!["env"], "env command"),
        (vec!["host"], "host command"),
        (vec!["machine"], "app-wide machine command"),
        (vec!["machine", "show"], "environment"),
    ] {
        let invocation = Invocation::new(root.clone(), &path);
        let loaded = invocation.load(&application).unwrap().unwrap();
        assert_eq!(loaded.node.req::<String>("value").unwrap(), expected);
    }
}

#[test]
fn an_application_named_env_can_choose_a_different_environment_file() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("machine.rhai"), "#{ value: 31 }").unwrap();
    fs::write(directory.path().join("env.rhai"), "#{ render: #{ value: host.value } }").unwrap();
    let application = ApplicationConfig::new("env")
        .directory(directory.path())
        .environment_file("machine")
        .environment_binding("host");
    let loaded = invocation("render").load(&application).unwrap().unwrap();
    assert_eq!(loaded.node.req::<i64>("value").unwrap(), 31);
}

#[test]
fn multiple_candidates_share_one_bootstrap_with_the_configured_environment() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("machine.machine-data"), "37").unwrap();
    fs::write(directory.path().join("render.rhai"), "#{ value: record(host.value) }").unwrap();
    fs::write(
        directory.path().join("atlas.rhai"),
        "let namespace = #{}; namespace.other = #{ value: record(host.value) }; namespace",
    )
    .unwrap();
    let environment_calls = Rc::new(Cell::new(0));
    let candidate_calls = Rc::new(Cell::new(0));
    let registry = default_format_parser_registry_builder()
        .add(DataParser(environment_calls.clone()))
        .unwrap()
        .build();
    let recorded_calls = candidate_calls.clone();
    let application = custom_environment(directory.path()).registry(registry).rhai_engine(
        move |engine, environment| {
            let value = environment.scope().get_value::<rhai::Map>("host").unwrap();
            assert_eq!(value["value"].clone().cast::<i64>(), 37);
            let recorded_calls = recorded_calls.clone();
            engine.register_fn("record", move |value: i64| {
                recorded_calls.set(recorded_calls.get() + 1);
                value
            });
        },
    );
    let invocation = Invocation::new(
        Command::new("atlas")
            .subcommand_required(true)
            .subcommands([Command::new("render"), Command::new("other")]),
        &["render"],
    );
    let loaded = invocation.load(&application).unwrap().unwrap();
    assert_eq!(loaded.node.req::<i64>("value").unwrap(), 37);
    assert_eq!(environment_calls.get(), 1);
    assert_eq!(candidate_calls.get(), 2);
}

#[test]
fn one_source_resolution_uses_local_environment_and_only_delegated_commands() {
    let directory = tempdir().unwrap();
    let conventional = directory.path().join("conventional");
    let target = directory.path().join("target");
    fs::create_dir(&conventional).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(conventional.join("source.pointer"), "../target").unwrap();
    fs::write(conventional.join("machine.rhai"), "#{ value: 41 }").unwrap();
    fs::write(conventional.join("render.rhai"), "throw \"local command is unused\";").unwrap();
    fs::write(target.join("machine.rhai"), "#{ value: 99 }").unwrap();
    fs::write(target.join("render.rhai"), "#{ value: host.value }").unwrap();
    let calls = Rc::new(Cell::new(0));
    let registry =
        default_format_parser_registry_builder().add(PointerParser(calls.clone())).unwrap().build();
    let application = custom_environment(&conventional).registry(registry);
    let loaded = invocation("render").load(&application).unwrap().unwrap();
    assert_eq!(loaded.node.req::<i64>("value").unwrap(), 41);
    assert_eq!(
        loaded.source.physical_path,
        target.join("render.rhai").canonicalize_path().unwrap()
    );
    assert_eq!(calls.get(), 1);

    fs::remove_file(conventional.join("machine.rhai")).unwrap();
    assert!(invocation("render").load(&application).is_err());
    assert_eq!(calls.get(), 2);
}

#[test]
fn direct_environment_and_explicit_rhai_selection_bypass_the_source_file() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("source.rhai"), "throw \"source is unused\";").unwrap();
    fs::write(directory.path().join("machine.rhai"), "#{ value: 43 }").unwrap();
    let path = directory.path().join("alternate.rhai");
    fs::write(&path, "#{ selected: #{ value: host.value } }").unwrap();
    let application = custom_environment(directory.path());
    let environment = application.load_environment().unwrap();
    let value = environment.scope().get_value::<rhai::Map>("host").unwrap();
    assert_eq!(value["value"].clone().cast::<i64>(), 43);
    let loaded = application.load_file(&path, KeyPath::from_keys(["selected"])).unwrap();
    assert_eq!(loaded.node.req::<i64>("value").unwrap(), 43);
    let mut argument = path.as_os_str().to_owned();
    argument.push("::selected");
    let value = Setup::standard("render")
        .config_source(|source| source.application(application))
        .into_bundle(|input: NumberConfig| input.value)
        .run_from([OsString::from("render"), "-C".into(), argument])
        .unwrap()
        .unwrap();
    assert_eq!(value, 43);
}

#[test]
fn legal_binding_identifiers_need_not_match_the_filename() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("machine.rhai"), "#{ value: 47 }").unwrap();
    for binding in ["host", "_host", "host_2"] {
        let environment = ApplicationConfig::new("atlas")
            .directory(directory.path())
            .environment_file("machine")
            .environment_binding(binding)
            .load_environment()
            .unwrap();
        assert!(environment.scope().contains(binding));
    }
}

#[test]
fn invalid_environment_stems_fail_before_source_or_payload_loading() {
    for stem in [
        "",
        ".",
        "..",
        "foo.rhai",
        "nested/machine",
        "nested\\machine",
        "source",
        "SOURCE",
        "atlas",
        "ATLAS",
        "machine:alternate",
        "machine?",
        "machine*",
        "machine|",
        "machine<",
        "machine>",
        "machine\"",
        "machine\n",
        "machine.",
        "machine ",
        "CON",
        "NUL",
        "cOm1",
        "LPT9",
        "COM¹",
        "COM²",
        "COM³",
        "LPT¹",
        "LPT²",
        "LPT³",
    ] {
        assert_invalid_before_loading(|application| application.environment_file(stem));
    }
}

#[test]
fn invalid_bindings_fail_before_source_or_payload_loading() {
    for binding in [
        "",
        "_",
        "__",
        "this",
        "let",
        "fn",
        "export",
        "sync",
        "true",
        "host::value",
        "host.value",
        "host-name",
        "9host",
        "host; throw \"injected\";",
    ] {
        assert_invalid_before_loading(|application| application.environment_binding(binding));
    }
}

#[test]
fn an_application_name_cannot_equal_its_configured_environment_stem() {
    let directory = tempdir().unwrap();
    let application =
        ApplicationConfig::new("machine").directory(directory.path()).environment_file("machine");
    assert!(application.load_environment().is_err());
    assert!(invocation("render").load(&application).is_err());
}

#[test]
fn invalid_settings_are_not_ignored_when_the_config_root_is_missing() {
    let directory = tempdir().unwrap();
    let missing = directory.path().join("missing");
    let applications = [
        ApplicationConfig::new("atlas").directory(&missing).environment_file("bad.rhai"),
        ApplicationConfig::new("atlas").directory(&missing).environment_binding("host::value"),
    ];
    for application in applications {
        let environment_error = application.load_environment().err().unwrap();
        assert!(environment_error.to_string().contains("environment"));
        let command_error = invocation("render").load(&application).err().unwrap();
        assert!(command_error.to_string().contains("environment"));
        let explicit_error = application
            .load_file(directory.path().join("missing.json"), KeyPath::new())
            .err()
            .unwrap();
        assert!(explicit_error.to_string().contains("environment"));
    }
}

fn assert_invalid_before_loading(configure: impl Fn(ApplicationConfig) -> ApplicationConfig) {
    let directory = tempdir().unwrap();
    let conventional = directory.path().join("config");
    let target = directory.path().join("target");
    fs::create_dir(&conventional).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(conventional.join("source.pointer"), "../target").unwrap();
    let input = directory.path().join("input.machine-data");
    fs::write(&input, "53").unwrap();
    let pointer_calls = Rc::new(Cell::new(0));
    let payload_calls = Rc::new(Cell::new(0));
    let registry = default_format_parser_registry_builder()
        .add(PointerParser(pointer_calls.clone()))
        .unwrap()
        .add(DataParser(payload_calls.clone()))
        .unwrap()
        .build();
    let application =
        configure(ApplicationConfig::new("atlas").directory(&conventional).registry(registry));
    assert!(application.load_environment().is_err());
    assert!(invocation("render").load(&application).is_err());
    assert!(application.load_file(&input, KeyPath::new()).is_err());
    assert_eq!(pointer_calls.get(), 0, "invalid settings must not resolve source files");
    assert_eq!(payload_calls.get(), 0, "invalid settings must not parse explicit data files");
}

fn custom_environment(directory: &Path) -> ApplicationConfig {
    ApplicationConfig::new("atlas")
        .directory(directory)
        .environment_file("machine")
        .environment_binding("host")
}

struct Invocation {
    root: Command,
    path: Vec<String>,
}

impl Invocation {
    fn new(root: Command, path: &[&str]) -> Self {
        Self {
            root,
            path: path.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    fn load(
        &self,
        application: &ApplicationConfig,
    ) -> Result<Option<LoadedCommandConfig>, CommandConfigError> {
        application.load_command(&self.root, &self.path)
    }
}

fn invocation(name: &str) -> Invocation {
    Invocation::new(
        Command::new("atlas").subcommand_required(true).subcommand(Command::new(name.to_owned())),
        &[name],
    )
}

struct DataParser(Rc<Cell<usize>>);

impl ConfigFormatParser for DataParser {
    fn id(&self) -> Format {
        Format::custom("machine-data").unwrap()
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["machine-data"]
    }
    fn parse_str(&self, source: &str) -> Result<Node, NodeError> {
        self.0.set(self.0.get() + 1);
        let value = source.parse::<i64>().map_err(|error| NodeError::new(error.to_string()))?;
        let mut node = Node::empty_map();
        node.set_value("value", value)?;
        Ok(node)
    }
}

struct PointerParser(Rc<Cell<usize>>);

impl ConfigFormatParser for PointerParser {
    fn id(&self) -> Format {
        Format::custom("pointer").unwrap()
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["pointer"]
    }
    fn parse_str(&self, source: &str) -> Result<Node, NodeError> {
        self.0.set(self.0.get() + 1);
        let mut node = Node::empty_map();
        node.set_value("directory", source)?;
        Ok(node)
    }
}
