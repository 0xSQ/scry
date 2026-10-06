use super::*;
use crate::node::Format;
use std::fs;
use tempfile::TempDir;

// ---------------------------------------------------------------------------------------------- //

fn write(dir: &Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}

#[test]
fn null_directory_is_optional_but_null_commands_are_invalid() {
    let node = Node::parse_str("#{ dir: () }", Format::Rhai).unwrap();
    let spec = RedirectSpec::from_node(&node).unwrap();
    assert!(spec.dir.is_none());
    assert!(spec.commands.is_empty());
    node.ensure_no_unknown_keys().unwrap();

    let node = Node::parse_str("#{ commands: () }", Format::Rhai).unwrap();
    assert!(RedirectSpec::from_node(&node).is_err());
}

#[test]
fn redirect_maps_reject_unknown_empty_values() {
    for extra in ["#{}", "[]"] {
        let mut node = Node::parse_str(r#"#{ dir: "configs" }"#, Format::Rhai).unwrap();
        node.set_node("commandz", Node::parse_str(extra, Format::Rhai).unwrap()).unwrap();
        let error = RedirectSpec::from_node(&node).unwrap_err();

        assert!(matches!(error, NodeError::UnknownKeys { ref paths }
            if *paths == [crate::KeyPath::from_keys(["commandz"])]));
    }
}

#[test]
fn redirect_strictness_does_not_depend_on_previous_reads() {
    let node = Node::parse_str(r#"#{ commandz: "invalid" }"#, Format::Rhai).unwrap();
    assert!(node.req::<u32>("commandz").is_err());

    let error = RedirectSpec::from_node(&node).unwrap_err();
    assert!(matches!(error, NodeError::UnknownKeys { ref paths }
        if *paths == [crate::KeyPath::from_keys(["commandz"])]));
}

#[test]
fn redirect_command_keys_remain_dynamic_and_literal() {
    let node = Node::parse_str(
        r#"#{ commands: #{ meta: "group.rhai", "meta.inspect": "specific.rhai", "odd key": "odd.rhai" } }"#,
        Format::Rhai,
    )
    .unwrap();
    let spec = RedirectSpec::from_node(&node).unwrap();

    assert!(spec.dir.is_none());
    assert_eq!(spec.commands.len(), 3);
    assert_eq!(spec.commands["meta"], "group.rhai");
    assert_eq!(spec.commands["meta.inspect"], "specific.rhai");
    assert_eq!(spec.commands["odd key"], "odd.rhai");

    let empty = Node::parse_str("#{ commands: #{} }", Format::Rhai).unwrap();
    let spec = RedirectSpec::from_node(&empty).unwrap();
    assert!(spec.commands.is_empty());
}

#[test]
fn per_command_file_wins_over_redirect() {
    let temp = TempDir::new().unwrap();
    let expected = write(temp.path(), "conjure.rhai", "#{}");
    write(temp.path(), "app.rhai", r#""somewhere/else""#);

    let resolution = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap();
    assert_eq!(resolution.path, Some(expected));
}

#[test]
fn string_shorthand_redirects_to_dir() {
    let temp = TempDir::new().unwrap();
    let real_dir = temp.path().join("real");
    fs::create_dir(&real_dir).unwrap();
    let expected = write(&real_dir, "conjure.rhai", "#{}");
    write(temp.path(), "app.rhai", r#""real""#);

    let resolution = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap();
    assert_eq!(resolution.path, Some(expected));
}

#[test]
fn config_export_works_as_redirect_value() {
    let temp = TempDir::new().unwrap();
    let real_dir = temp.path().join("real");
    fs::create_dir(&real_dir).unwrap();
    let expected = write(&real_dir, "generate.rhai", "#{}");
    write(temp.path(), "app.rhai", r#"export let config = "real";"#);

    let resolution = resolve_in_dir(temp.path(), "app", &["generate"]).unwrap();
    assert_eq!(resolution.path, Some(expected));
}

#[test]
fn explicit_entry_wins_over_dir() {
    let temp = TempDir::new().unwrap();
    let real_dir = temp.path().join("real");
    fs::create_dir(&real_dir).unwrap();
    write(&real_dir, "conjure.rhai", "#{}");
    let special = write(temp.path(), "special.rhai", "#{}");
    write(temp.path(), "app.rhai", r#"#{ dir: "real", commands: #{ conjure: "special.rhai" } }"#);

    let resolution = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap();
    assert_eq!(resolution.path, Some(special));
}

#[test]
fn most_specific_command_entry_wins() {
    let temp = TempDir::new().unwrap();
    let group = write(temp.path(), "group.rhai", "#{}");
    let specific = write(temp.path(), "specific.rhai", "#{}");
    write(
        temp.path(),
        "app.rhai",
        r#"#{ commands: #{ meta: "group.rhai", "meta.inspect": "specific.rhai" } }"#,
    );

    let resolution = resolve_in_dir(temp.path(), "app", &["meta", "inspect"]).unwrap();
    assert_eq!(resolution.path, Some(specific));

    let resolution = resolve_in_dir(temp.path(), "app", &["meta", "comfy"]).unwrap();
    assert_eq!(resolution.path, Some(group));
}

#[test]
fn missing_entry_target_is_an_error() {
    let temp = TempDir::new().unwrap();
    write(temp.path(), "app.rhai", r#"#{ commands: #{ conjure: "nowhere.rhai" } }"#);

    let err = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap_err();
    assert!(matches!(err, RedirectError::MissingTarget { .. }));
}

#[test]
fn exhausted_chain_returns_none_with_trail() {
    let temp = TempDir::new().unwrap();
    write(temp.path(), "app.rhai", r#"#{ commands: #{ other: "app.rhai" } }"#);

    let resolution = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap();
    assert_eq!(resolution.path, None);
    assert_eq!(resolution.tried.len(), 3);
    assert!(resolution.tried[1].contains("no entry for 'conjure'"));
    assert!(resolution.tried[2].contains("declares no 'dir'"));
}

#[test]
fn no_redirect_file_returns_none() {
    let temp = TempDir::new().unwrap();

    let resolution = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap();
    assert_eq!(resolution.path, None);
    assert_eq!(resolution.tried.len(), 2);
}

#[test]
fn unknown_keys_are_rejected() {
    let temp = TempDir::new().unwrap();
    write(temp.path(), "app.rhai", r#"#{ dir: "real", commandz: "oops" }"#);

    let err = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap_err();
    assert!(matches!(err, RedirectError::InvalidRedirectFile { .. }));
}

#[cfg(feature = "format-json")]
#[test]
fn json_redirect_file_works() {
    let temp = TempDir::new().unwrap();
    let real_dir = temp.path().join("real");
    fs::create_dir(&real_dir).unwrap();
    let expected = write(&real_dir, "conjure.json", r#"{"conjure": {}}"#);
    write(temp.path(), "app.json", r#""real""#);

    let resolution = resolve_in_dir(temp.path(), "app", &["conjure"]).unwrap();
    assert_eq!(resolution.path, Some(expected));
}

#[test]
fn tilde_paths_resolve_against_home() {
    let home = dirs::home_dir().unwrap();
    let resolved = resolve_redirect_path(Path::new("base"), "~/configs");
    assert_eq!(resolved, home.join("configs"));
}
