use super::*;
use crate::node::Format;
#[cfg(any(unix, windows))]
use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;
use tempfile::TempDir;

// ---------------------------------------------------------------------------------------------- //
// Shared Fixtures and Assertions

fn create_test_files(dir: &TempDir, names: &[&str]) {
    for name in names {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, "test content").unwrap();
    }
}

#[cfg(unix)]
fn opaque_component(tag: u8) -> OsString {
    OsString::from_vec(vec![b'o', 0xff, tag])
}

#[cfg(windows)]
fn opaque_component(tag: u8) -> OsString {
    OsString::from_wide(&[b'o' as u16, 0xd800, tag as u16])
}

#[cfg(unix)]
fn opaque_file_name(tag: u8) -> OsString {
    OsString::from_vec(vec![b'o', 0xff, tag, b'.', b't', b'x', b't'])
}

#[cfg(windows)]
fn opaque_file_name(tag: u8) -> OsString {
    OsString::from_wide(&[
        b'o' as u16,
        0xd800,
        tag as u16,
        b'.' as u16,
        b't' as u16,
        b'x' as u16,
        b't' as u16,
    ])
}

#[cfg(any(unix, windows))]
fn create_opaque_file(dir: &Path, tag: u8) -> PathBuf {
    let path = dir.join(opaque_file_name(tag));
    fs::write(&path, "opaque file").unwrap();
    path
}

#[cfg(any(unix, windows))]
fn create_opaque_dir(dir: &Path, tag: u8) -> PathBuf {
    let path = dir.join(opaque_component(tag));
    fs::create_dir(&path).unwrap();
    path
}

#[cfg(any(unix, windows))]
fn assert_non_unicode_error<T>(result: Result<T, FilesError>, expected: &Path) {
    match result {
        Err(FilesError::NonUnicodePath { path }) => assert_eq!(path, expected),
        Err(error) => panic!("expected NonUnicodePath for {expected:?}, got {error:?}"),
        Ok(_) => panic!("expected NonUnicodePath for {expected:?}, got success"),
    }
}

fn relative_path_strings(dir: &TempDir, paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|path| {
            let relative = path.strip_prefix(dir.path()).unwrap();
            normalize_path_separators(relative.to_str().unwrap())
        })
        .collect()
}

fn exact_root(path: &str) -> PathPatternSpec {
    PathPatternSpec {
        path: path.to_string(),
        syntax: PatternSyntax::Exact,
        must_exist: true,
        recursive: true,
    }
}

fn wildcard_root(path: &str) -> PathPatternSpec {
    PathPatternSpec {
        path: path.to_string(),
        syntax: PatternSyntax::Wildcard,
        must_exist: false,
        recursive: true,
    }
}

fn source_with_root(path: &str) -> SourceSpec {
    SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(exact_root(path)),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    }
}

fn source_with_root_and_prune(root: &str, prune: &str) -> SourceSpec {
    SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(exact_root(root)),
            prune: OneOrMany::one(exact_root(prune)),
        }),
        where_: None,
        sort: PathSort::Natural,
    }
}

fn source_with_root_and_path_exclude(root: &str, exclude: &str) -> SourceSpec {
    SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(exact_root(root)),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            path: Some(AttrRuleSpec {
                include: OneOrMany::default(),
                exclude: OneOrMany::one(TextPatternSpec::auto_detect(exclude.to_string())),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    }
}

// ---------------------------------------------------------------------------------------------- //
// Configuration Parsing and Serialization

#[test]
fn path_pattern_string_no_wildcards() {
    let node = Node::parse_str("\"some/file.txt\"", Format::Rhai).unwrap();
    let pp: PathPatternSpec = node.as_type().unwrap();
    assert_eq!(pp.path, "some/file.txt");
    assert_eq!(pp.syntax, PatternSyntax::Exact);
    assert!(pp.must_exist);
    assert!(pp.recursive);
}

#[test]
fn path_pattern_string_with_asterisk() {
    let node = Node::parse_str("\"*.txt\"", Format::Rhai).unwrap();
    let pp: PathPatternSpec = node.as_type().unwrap();
    assert_eq!(pp.path, "*.txt");
    assert_eq!(pp.syntax, PatternSyntax::Wildcard);
    assert!(!pp.must_exist);
}

#[test]
fn path_pattern_string_with_question_mark() {
    let node = Node::parse_str("\"file?.txt\"", Format::Rhai).unwrap();
    let pp: PathPatternSpec = node.as_type().unwrap();
    assert_eq!(pp.syntax, PatternSyntax::Wildcard);
}

#[test]
fn path_pattern_explicit_exact_defaults() {
    let node =
        Node::parse_str(r#"#{ path: "some/file.txt", syntax: "exact" }"#, Format::Rhai).unwrap();
    let pp: PathPatternSpec = node.as_type().unwrap();
    assert_eq!(pp.path, "some/file.txt");
    assert_eq!(pp.syntax, PatternSyntax::Exact);
    assert!(pp.must_exist); // Default for exact.
    assert!(pp.recursive);
}

#[test]
fn path_pattern_explicit_defaults_to_exact() {
    let node = Node::parse_str(r#"#{ path: "some/file.txt" }"#, Format::Rhai).unwrap();
    let pp: PathPatternSpec = node.as_type().unwrap();
    assert_eq!(pp.path, "some/file.txt");
    assert_eq!(pp.syntax, PatternSyntax::Exact);
    assert!(pp.must_exist); // Default for exact.
    assert!(pp.recursive);
}

#[test]
fn path_pattern_explicit_wildcard_defaults() {
    let node =
        Node::parse_str(r#"#{ path: "**/*.rs", syntax: "wildcard" }"#, Format::Rhai).unwrap();
    let pp: PathPatternSpec = node.as_type().unwrap();
    assert_eq!(pp.syntax, PatternSyntax::Wildcard);
    assert!(!pp.must_exist); // Default for wildcard.
}

#[test]
fn path_pattern_explicit_overrides() {
    let node = Node::parse_str(
        r#"#{ path: "src", syntax: "exact", must_exist: false, recursive: false }"#,
        Format::Rhai,
    )
    .unwrap();
    let pp: PathPatternSpec = node.as_type().unwrap();
    assert!(!pp.must_exist);
    assert!(!pp.recursive);
}

#[test]
fn path_pattern_auto_detect_helper_sets_syntax_and_defaults() {
    let exact = PathPatternSpec::auto_detect("README.md".to_string());
    assert_eq!(exact.path, "README.md");
    assert_eq!(exact.syntax, PatternSyntax::Exact);
    assert!(exact.must_exist);
    assert!(exact.recursive);

    let wildcard = PathPatternSpec::auto_detect("docs/*.md".to_string());
    assert_eq!(wildcard.path, "docs/*.md");
    assert_eq!(wildcard.syntax, PatternSyntax::Wildcard);
    assert!(!wildcard.must_exist);
    assert!(wildcard.recursive);
}

#[test]
fn path_pattern_map_unknown_key_errors() {
    let node =
        Node::parse_str(r#"#{ path: "file.txt", syntax: "exact", extra: true }"#, Format::Rhai)
            .unwrap();
    let result: Result<PathPatternSpec, _> = node.as_type();
    assert!(result.is_err());
}

#[test]
fn path_pattern_map_pattern_key_errors() {
    let node = Node::parse_str(r#"#{ pattern: "file.txt" }"#, Format::Rhai).unwrap();
    let result: Result<PathPatternSpec, _> = node.as_type();
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------------------------- //
// TextPatternSpec Parsing Tests

#[test]
fn text_pattern_string_exact() {
    let node = Node::parse_str("\"README.md\"", Format::Rhai).unwrap();
    let tp: TextPatternSpec = node.as_type().unwrap();
    assert_eq!(tp.pattern, "README.md");
    assert_eq!(tp.syntax, PatternSyntax::Exact);
}

#[test]
fn text_pattern_string_wildcard() {
    let node = Node::parse_str("\"README*\"", Format::Rhai).unwrap();
    let tp: TextPatternSpec = node.as_type().unwrap();
    assert_eq!(tp.syntax, PatternSyntax::Wildcard);
}

#[test]
fn text_pattern_explicit_map() {
    let node =
        Node::parse_str(r#"#{ pattern: "README*", syntax: "wildcard" }"#, Format::Rhai).unwrap();
    let tp: TextPatternSpec = node.as_type().unwrap();
    assert_eq!(tp.pattern, "README*");
    assert_eq!(tp.syntax, PatternSyntax::Wildcard);
}

#[test]
fn text_pattern_explicit_map_auto_detects() {
    let node = Node::parse_str(r#"#{ pattern: "README*" }"#, Format::Rhai).unwrap();
    let tp: TextPatternSpec = node.as_type().unwrap();
    assert_eq!(tp.syntax, PatternSyntax::Wildcard);
}

#[test]
fn text_pattern_glob_syntax_errors() {
    let node = Node::parse_str(r#"#{ pattern: "[a-z]*", syntax: "glob" }"#, Format::Rhai).unwrap();
    let result: Result<TextPatternSpec, _> = node.as_type();
    assert!(result.is_err());
}

#[test]
fn text_pattern_auto_detect_helper() {
    let exact = TextPatternSpec::auto_detect("README.md".to_string());
    assert_eq!(exact.syntax, PatternSyntax::Exact);

    let wildcard = TextPatternSpec::auto_detect("README*".to_string());
    assert_eq!(wildcard.syntax, PatternSyntax::Wildcard);
}

// ---------------------------------------------------------------------------------------------- //
// AttrRuleSpec Parsing Tests

#[test]
fn attr_rule_string_shorthand() {
    let node = Node::parse_str("\"rs\"", Format::Rhai).unwrap();
    let rule: AttrRuleSpec = node.as_type().unwrap();
    assert_eq!(rule.include.len(), 1);
    assert_eq!(rule.include[0].pattern, "rs");
    assert!(rule.exclude.is_empty());
}

#[test]
fn attr_rule_array_shorthand() {
    let node = Node::parse_str(r#"["rs", "toml"]"#, Format::Rhai).unwrap();
    let rule: AttrRuleSpec = node.as_type().unwrap();
    assert_eq!(rule.include.len(), 2);
    assert!(rule.exclude.is_empty());
}

#[test]
fn attr_rule_map_full() {
    let node =
        Node::parse_str(r#"#{ include: ["rs", "toml"], exclude: ["bak"] }"#, Format::Rhai).unwrap();
    let rule: AttrRuleSpec = node.as_type().unwrap();
    assert_eq!(rule.include.len(), 2);
    assert_eq!(rule.exclude.len(), 1);
}

#[test]
fn attr_rule_map_exclude_only() {
    let node = Node::parse_str(r#"#{ exclude: "bak" }"#, Format::Rhai).unwrap();
    let rule: AttrRuleSpec = node.as_type().unwrap();
    assert!(rule.include.is_empty());
    assert_eq!(rule.exclude.len(), 1);
}

#[test]
fn attr_rule_empty_map() {
    let node = Node::parse_str("#{}", Format::Rhai).unwrap();
    let rule: AttrRuleSpec = node.as_type().unwrap();
    assert!(rule.include.is_empty());
    assert!(rule.exclude.is_empty());
}

// ---------------------------------------------------------------------------------------------- //
// PathSort Parsing Tests

#[test]
fn path_sort_parses_supported_modes() {
    let natural = Node::parse_str("\"natural\"", Format::Rhai).unwrap();
    let lexicographic = Node::parse_str("\"lexicographic\"", Format::Rhai).unwrap();

    assert_eq!(natural.as_type::<PathSort>().unwrap(), PathSort::Natural);
    assert_eq!(lexicographic.as_type::<PathSort>().unwrap(), PathSort::Lexicographic);
}

#[test]
fn path_sort_rejects_unknown_mode() {
    let node = Node::parse_str("\"locale\"", Format::Rhai).unwrap();
    let error = node.as_type::<PathSort>().unwrap_err().to_string();

    assert!(error.contains("natural"));
    assert!(error.contains("lexicographic"));
}

#[test]
fn path_sort_description_remains_files_config_specific() {
    assert_eq!(PathSort::describe().type_label(), "path ordering mode");
}

#[test]
fn source_spec_defaults_to_natural_sort() {
    assert_eq!(SourceSpec::default().sort, PathSort::Natural);
}

// ---------------------------------------------------------------------------------------------- //
// SourceSpec Parsing Tests

#[test]
fn source_spec_string_shorthand() {
    let node = Node::parse_str("\"some/path\"", Format::Rhai).unwrap();
    let spec: SourceSpec = node.as_type().unwrap();
    assert_eq!(spec.sort, PathSort::Natural);
    let from = spec.from.unwrap();
    assert_eq!(from.root.len(), 1);
    assert_eq!(from.root[0].path, "some/path");
    assert_eq!(from.root[0].syntax, PatternSyntax::Exact);
    assert!(spec.where_.is_none());
}

#[test]
fn source_spec_string_wildcard_shorthand() {
    let node = Node::parse_str("\"*.rhai\"", Format::Rhai).unwrap();
    let spec: SourceSpec = node.as_type().unwrap();
    let from = spec.from.unwrap();
    assert_eq!(from.root[0].syntax, PatternSyntax::Wildcard);
}

#[test]
fn source_spec_map_from_and_where() {
    let node = Node::parse_str(
        r#"#{ from: #{ root: ["src/", "lib/"], prune: "target" }, where: #{ ext: ["rs", "toml"] } }"#,
        Format::Rhai,
    )
    .unwrap();
    let spec: SourceSpec = node.as_type().unwrap();
    assert_eq!(spec.sort, PathSort::Natural);
    let from = spec.from.unwrap();
    assert_eq!(from.root.len(), 2);
    assert_eq!(from.prune.len(), 1);
    let where_ = spec.where_.unwrap();
    assert!(where_.ext.is_some());
}

#[test]
fn source_spec_where_only() {
    let node = Node::parse_str(r#"#{ where: #{ ext: "rs" } }"#, Format::Rhai).unwrap();
    let spec: SourceSpec = node.as_type().unwrap();
    assert!(spec.from.is_none());
    assert!(spec.where_.is_some());
}

#[test]
fn source_spec_from_string_shorthand() {
    let node = Node::parse_str(r#"#{ from: "/some/dir" }"#, Format::Rhai).unwrap();
    let spec: SourceSpec = node.as_type().unwrap();
    let from = spec.from.unwrap();
    assert_eq!(from.root.len(), 1);
    assert_eq!(from.root[0].path, "/some/dir");
}

#[test]
fn source_spec_from_array_shorthand() {
    let node = Node::parse_str(r#"#{ from: ["src", "tests"] }"#, Format::Rhai).unwrap();
    let spec: SourceSpec = node.as_type().unwrap();
    let from = spec.from.unwrap();
    assert_eq!(from.root.len(), 2);
    assert_eq!(from.root[0].path, "src");
    assert_eq!(from.root[1].path, "tests");
}

#[test]
fn source_spec_from_with_omit_errors() {
    let node =
        Node::parse_str(r#"#{ from: #{ root: "src/", omit: "**/*.generated.rs" } }"#, Format::Rhai)
            .unwrap();
    let result: Result<SourceSpec, _> = node.as_type();
    assert!(result.is_err());
}

#[test]
fn source_spec_unknown_key_errors() {
    let node = Node::parse_str(r#"#{ from: "src/", unknown: true }"#, Format::Rhai).unwrap();
    let result: Result<SourceSpec, _> = node.as_type();
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------------------------- //
// WhereSpec Parsing Tests

#[cfg(feature = "format-json")]
#[test]
fn where_spec_full() {
    let node = Node::parse_str(
        r#"{ "case": "sensitive", "path": "src/*", "name": "Cargo.toml", "stem": ["main", "lib"], "ext": { "include": ["rs"], "exclude": ["bak"] } }"#,
        Format::Json,
    )
    .unwrap();
    let w: WhereSpec = node.as_type().unwrap();
    assert_eq!(w.case, CaseMode::Sensitive);
    assert!(w.path.is_some());
    assert!(w.name.is_some());
    assert!(w.stem.is_some());
    assert!(w.ext.is_some());
}

#[test]
fn where_spec_defaults() {
    let node = Node::parse_str(r#"#{ ext: "rs" }"#, Format::Rhai).unwrap();
    let w: WhereSpec = node.as_type().unwrap();
    assert_eq!(w.case, CaseMode::Insensitive);
    assert!(w.path.is_none());
    assert!(w.name.is_none());
    assert!(w.stem.is_none());
    assert!(w.ext.is_some());
}

// ---------------------------------------------------------------------------------------------- //
// Files Parsing Tests

#[test]
fn files_string_shorthand() {
    let node = Node::parse_str(r#""src/*.rs""#, Format::Rhai).unwrap();
    let f: Files = node.as_type().unwrap();
    assert_eq!(f.sources.len(), 1);
    assert_eq!(f.sources[0].from.as_ref().unwrap().root[0].syntax, PatternSyntax::Wildcard);
}

#[test]
fn files_array_shorthand() {
    let node = Node::parse_str(r#"["src/*.rs", "docs/*.md"]"#, Format::Rhai).unwrap();
    let f: Files = node.as_type().unwrap();
    assert_eq!(f.sources.len(), 2);
}

#[test]
fn files_single_source_map() {
    let node = Node::parse_str(r#"#{ from: "src", where: #{ ext: "rs" } }"#, Format::Rhai).unwrap();
    let f: Files = node.as_type().unwrap();
    assert_eq!(f.sources.len(), 1);
    assert!(f.sources[0].where_.is_some());
}

#[test]
fn files_full_config() {
    let node = Node::parse_str(
        r#"#{ sources: [#{ from: "src/", where: #{ ext: ["rs"] } }, "*.rhai"] }"#,
        Format::Rhai,
    )
    .unwrap();
    let f: Files = node.as_type().unwrap();
    assert_eq!(f.sources.len(), 2);
    assert!(f.sources[0].from.is_some());
    assert!(f.sources[0].where_.is_some());
    assert!(f.sources[1].from.is_some());
    assert!(f.sources[1].where_.is_none());
}

#[test]
fn files_empty_sources() {
    let node = Node::parse_str("#{ sources: [] }", Format::Rhai).unwrap();
    let f: Files = node.as_type().unwrap();
    assert!(f.sources.is_empty());
}

#[test]
fn files_empty_map_is_single_implicit_source() {
    let node = Node::parse_str("#{}", Format::Rhai).unwrap();
    let files: Files = node.as_type().unwrap();
    assert_eq!(files.sources.len(), 1);
    assert!(files.sources[0].from.is_none());
}

#[test]
fn source_spec_parses_explicit_lexicographic_sort() {
    let node =
        Node::parse_str(r#"#{ from: "generated", sort: "lexicographic" }"#, Format::Rhai).unwrap();
    let spec: SourceSpec = node.as_type().unwrap();

    assert_eq!(spec.sort, PathSort::Lexicographic);
}

#[test]
fn source_spec_canonical_round_trip_preserves_explicit_sort() {
    let node =
        Node::parse_str(r#"#{ from: "generated", sort: "lexicographic" }"#, Format::Rhai).unwrap();
    let spec: SourceSpec = node.as_type().unwrap();

    let json = spec.to_node().unwrap().to_string_as(Format::Json).unwrap();
    assert!(json.contains(r#""sort": "lexicographic""#));
    let reparsed: SourceSpec = Node::parse_str(&json, Format::Json).unwrap().as_type().unwrap();

    assert_eq!(reparsed.sort, PathSort::Lexicographic);
}

#[test]
fn files_round_trip_through_canonical_json() {
    let node = Node::parse_str(
        r#"#{ from: #{ root: #{ path: "images", recursive: false } }, where: #{ ext: ["png", "webp"] } }"#,
        Format::Rhai,
    )
    .unwrap();
    let files: Files = node.as_type().unwrap();

    let json = files.to_node().unwrap().to_string_as(Format::Json).unwrap();
    assert!(json.contains(r#""sort": "natural""#));
    let reparsed: Files = Node::parse_str(&json, Format::Json).unwrap().as_type().unwrap();

    let source = &reparsed.sources[0];
    let root = &source.from.as_ref().unwrap().root[0];
    assert_eq!(root.path, "images");
    assert!(!root.recursive);
    assert_eq!(source.sort, PathSort::Natural);
    assert_eq!(source.where_.as_ref().unwrap().ext.as_ref().unwrap().include.len(), 2);
}

// ---------------------------------------------------------------------------------------------- //
// Configuration Helper Behavior

#[test]
fn escape_wildcard_for_globset_escapes_braces_and_brackets() {
    assert_eq!(escape_wildcard_for_globset("{a}[b]"), "[{]a[}][[]b[]]");
}

#[test]
fn escape_wildcard_for_globset_keeps_plain_wildcards() {
    assert_eq!(escape_wildcard_for_globset("**/*.txt"), "**/*.txt");
}

// ---------------------------------------------------------------------------------------------- //
// Exact Roots and Base Directories

#[test]
fn adapt_pattern_joins_relative_with_base_dir() {
    let base = Path::new("/some/base");
    let result = adapt_pattern(Some(base), "**/*.txt").unwrap();
    assert_eq!(result, "/some/base/**/*.txt");
}

#[cfg(not(windows))]
#[test]
fn adapt_pattern_returns_absolute_unchanged() {
    let result = adapt_pattern(Some(Path::new("/base")), "/abs/**/*.rs").unwrap();
    assert_eq!(result, "/abs/**/*.rs");
}

#[test]
fn adapt_pattern_errors_without_base_dir() {
    let result = adapt_pattern(None, "relative/*.txt");
    assert!(result.is_err());
}

#[test]
fn adapt_path_uses_an_empty_implicit_root_without_repeating_the_base() {
    let base = Path::new("relative/base");

    let result = adapt_path(Some(base), Path::new("")).unwrap();

    assert_eq!(result, base);
}

// Base-directory behavior.

#[test]
fn locate_no_base_dir_relative_path_errors() {
    let spec = source_with_root("relative/file.txt");

    assert!(matches!(
        spec.locate(None::<&Path>),
        Err(FilesError::MissingBaseDir { path }) if path == "relative/file.txt"
    ));
}

#[test]
fn locate_no_base_dir_absolute_path_works() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["file.txt"]);
    let abs_path = dir.path().join("file.txt");

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: abs_path.to_str().unwrap().to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(None::<&Path>).unwrap();
    assert_eq!(result, [abs_path]);
}
// Implicit-root behavior.

#[test]
fn locate_implicit_from_uses_base_dir() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["a.rs", "b.txt"]);

    // No from spec - should default to walking base_dir.
    let spec = SourceSpec {
        from: None,
        where_: Some(WhereSpec {
            ext: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::exact("rs".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["a.rs"]);
}

#[test]
fn locate_implicit_from_no_base_dir_errors() {
    let spec = SourceSpec {
        from: None,
        where_: None,
        sort: PathSort::Natural,
    };

    assert!(matches!(
        spec.locate(None::<&Path>),
        Err(FilesError::MissingBaseDir { path }) if path == "<implicit root>"
    ));
}

#[test]
fn locate_empty_from_defaults_to_base_dir() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["a.rs", "b.txt", "sub/c.rs"]);

    // from is Some but root is empty - should default to walking base_dir.
    let spec = SourceSpec {
        from: Some(FromSpec::default()),
        where_: Some(WhereSpec {
            ext: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::exact("rs".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["a.rs", "sub/c.rs"]);
}

#[test]
fn locate_relative_exact_file_with_base_dir() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["file.txt"]);
    let spec = source_with_root("file.txt");

    let result = spec.locate(Some(dir.path())).unwrap();

    assert_eq!(relative_path_strings(&dir, &result), ["file.txt"]);
}

#[test]
fn locate_root_missing_must_exist_errors() {
    let dir = TempDir::new().unwrap();
    let spec = source_with_root("nonexistent.txt");
    let expected = dir.path().join("nonexistent.txt");

    assert!(matches!(
        spec.locate(Some(dir.path())),
        Err(FilesError::RootPathNotFound { path }) if path == expected
    ));
}

#[test]
fn locate_root_missing_not_must_exist_ok() {
    let dir = TempDir::new().unwrap();
    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "nonexistent.txt".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: false,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), Vec::<String>::new());
}

#[test]
fn locate_root_dir_recursive() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/a.txt", "d/sub/b.txt", "d/sub/deep/c.txt"]);
    let spec = source_with_root("d");
    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(
        relative_path_strings(&dir, &result),
        ["d/a.txt", "d/sub/b.txt", "d/sub/deep/c.txt"]
    );
}

#[test]
fn locate_root_dir_non_recursive() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/a.txt", "d/b.txt", "d/sub/c.txt"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: false,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/a.txt", "d/b.txt"]);
}

// ---------------------------------------------------------------------------------------------- //
// Wildcard Roots

#[test]
fn locate_root_wildcard_expansion() {
    let dir = TempDir::new().unwrap();
    create_test_files(
        &dir,
        &[
            "subdir/c.txt",
            "subdir/a.txt",
            "subdir/b.txt",
            "subdir/a.rs",
        ],
    );

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(wildcard_root("subdir/*.txt")),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();

    assert_eq!(
        relative_path_strings(&dir, &result),
        ["subdir/a.txt", "subdir/b.txt", "subdir/c.txt"]
    );
}

#[test]
fn locate_root_wildcard_no_matches_ok() {
    let dir = TempDir::new().unwrap();

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "*.nonexistent".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert!(result.is_empty());
}

#[test]
fn locate_root_wildcard_must_exist_errors() {
    let dir = TempDir::new().unwrap();

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "*.nonexistent".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    assert!(matches!(
        spec.locate(Some(dir.path())),
        Err(FilesError::WildcardMustExist { pattern }) if pattern == "*.nonexistent"
    ));
}

#[test]
fn wildcard_must_exist_is_checked_after_filtering_and_pruning() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["bucket/item.txt"]);
    let required_wildcard = || PathPatternSpec {
        must_exist: true,
        ..wildcard_root("bucket*")
    };

    let filtered = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(required_wildcard()),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            ext: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::exact("rs".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    assert!(matches!(
        filtered.locate(Some(dir.path())),
        Err(FilesError::WildcardMustExist { pattern }) if pattern == "bucket*"
    ));

    let pruned = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(required_wildcard()),
            prune: OneOrMany::one(exact_root("bucket")),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    assert!(matches!(
        pruned.locate(Some(dir.path())),
        Err(FilesError::WildcardMustExist { pattern }) if pattern == "bucket*"
    ));
}

#[cfg(any(unix, windows))]
#[test]
fn wildcard_must_exist_counts_only_accepted_unicode_files() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("bucket")).unwrap();
    let opaque = create_opaque_file(&dir.path().join("bucket"), b'm');
    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "bucket*".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    assert_non_unicode_error(spec.locate_with(Some(dir.path()), OnError::Fail), &opaque);

    let result = spec.locate_with(Some(dir.path()), OnError::Ignore);
    assert!(matches!(result, Err(FilesError::WildcardMustExist { .. })));
}

// Literal root-pattern syntax.

#[test]
fn locate_wildcard_character_class_syntax_is_literal() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["file.mp3", "file.mp4", "file.mp[34]"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "*.mp[34]".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["file.mp[34]"]);
}

#[test]
fn locate_braces_are_literal() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["a.rs", "b.toml", "c.txt", "{rs,toml}"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "*.{rs,toml}".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), Vec::<String>::new());
}
// ---------------------------------------------------------------------------------------------- //
// Traversal and Recursion

#[test]
fn locate_root_wildcard_matching_directory() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["proj-a/x.rs", "proj-a/y.rs", "proj-b/z.rs", "other.txt"]);

    // Glob matches directories proj-a and proj-b; they should be walked.
    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "proj-*".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["proj-a/x.rs", "proj-a/y.rs", "proj-b/z.rs"]);
}

#[test]
fn locate_root_wildcard_matching_directory_with_prune() {
    let dir = TempDir::new().unwrap();
    create_test_files(
        &dir,
        &[
            "proj-a/src/x.rs",
            "proj-a/target/debug.rs",
            "proj-b/src/y.rs",
        ],
    );

    // Use an exact prune path for the target directory. (Wildcard prune patterns like
    // `**/target` match directory paths, not the file paths inside them, so they don't
    // filter descendants via is_excluded. This is a known limitation.)
    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "proj-*".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: true,
            }),
            prune: OneOrMany::one(PathPatternSpec {
                path: "proj-a/target".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: false,
                recursive: true,
            }),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["proj-a/src/x.rs", "proj-b/src/y.rs"]);
}

#[test]
fn locate_root_wildcard_matching_directory_with_where() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["proj/a.rs", "proj/b.toml", "proj/c.txt"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "proj*".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            ext: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec {
                    pattern: "rs".to_string(),
                    syntax: PatternSyntax::Exact,
                }),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["proj/a.rs"]);
}

#[test]
fn locate_root_wildcard_mixed_files_and_dirs() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["alpha/nested.txt", "alpha.txt"]);

    // Pattern "alpha*" matches both the file "alpha.txt" and the directory "alpha".
    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "alpha*".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["alpha/nested.txt", "alpha.txt"]);
}

#[test]
fn locate_root_wildcard_matching_directory_non_recursive() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["proj/top.rs", "proj/sub/deep.rs"]);

    // recursive=false: wildcard-matched directory should only collect direct children.
    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "proj*".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: false,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["proj/top.rs"]);
}

// ---------------------------------------------------------------------------------------------- //
// Pruning

#[test]
fn locate_prune_path_file() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/keep.txt", "d/remove.txt"]);

    let spec = source_with_root_and_prune("d", "d/remove.txt");
    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/keep.txt"]);
}

#[test]
fn locate_prune_dir_recursive() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/a.txt", "d/sub/b.txt", "d/sub/deep/c.txt"]);

    let spec = source_with_root_and_prune("d", "d/sub");
    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/a.txt"]);
}

#[test]
fn locate_prune_dir_non_recursive() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/a.txt", "d/sub/b.txt", "d/sub/deep/c.txt"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::one(PathPatternSpec {
                path: "d/sub".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: false,
            }),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/a.txt", "d/sub/deep/c.txt"]);
}

#[test]
fn locate_prune_wildcard_filters_files_at_multiple_depths() {
    let dir = TempDir::new().unwrap();
    create_test_files(
        &dir,
        &[
            "d/keep.rs",
            "d/remove.tmp",
            "d/sub/also-remove.tmp",
            "d/sub/keep.txt",
        ],
    );

    // Prune wildcard patterns match against base_dir-relative paths.
    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::one(PathPatternSpec {
                path: "**/*.tmp".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: true,
            }),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/keep.rs", "d/sub/keep.txt"]);
}

#[test]
fn locate_prune_missing_not_must_exist_ok() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/a.txt"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::one(PathPatternSpec {
                path: "d/nonexistent".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: false,
                recursive: true,
            }),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/a.txt"]);
}

#[test]
fn locate_prune_missing_must_exist_errors() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/a.txt"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::one(PathPatternSpec {
                path: "d/nonexistent".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let expected = dir.path().join("d/nonexistent");
    assert!(matches!(
        spec.locate(Some(dir.path())),
        Err(FilesError::PrunePathNotFound { path }) if path == expected
    ));
}

// ---------------------------------------------------------------------------------------------- //
// Additional Pruning Behavior

#[test]
fn prune_wildcard_absolute_pattern() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/keep.rs", "d/remove.tmp"]);

    // An absolute prune wildcard should still match even when base_dir is set.
    let abs_pattern = format!("{}/**/*.tmp", dir.path().display());
    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::one(PathPatternSpec {
                path: abs_pattern,
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: true,
            }),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/keep.rs"]);
}

#[test]
fn pruning_skips_root_entirely() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/a.txt", "d/sub/b.txt"]);

    // Root "d" is itself pruned.
    let spec = source_with_root_and_prune("d", "d");
    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), Vec::<String>::new());
}
// ---------------------------------------------------------------------------------------------- //
// Where Filtering

#[test]
fn locate_where_ext_include() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/video.mp4", "d/video.webm", "d/image.png"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            ext: Some(AttrRuleSpec {
                include: OneOrMany::new(vec![
                    TextPatternSpec::exact("mp4".to_string()),
                    TextPatternSpec::exact("webm".to_string()),
                ]),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/video.mp4", "d/video.webm"]);
}

#[test]
fn locate_where_ext_case_insensitive() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/video.MP4", "d/audio.mp4"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            ext: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::exact("mp4".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/audio.mp4", "d/video.MP4"]);
}

#[test]
fn locate_where_ext_leading_dot() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/video.mp4", "d/image.png"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            ext: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::exact(".mp4".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/video.mp4"]);
}

#[test]
fn locate_where_ext_empty_matches_no_extension() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/with_ext.mp4", "d/no_ext"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            ext: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::exact("".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/no_ext"]);
}

#[test]
fn locate_where_ext_exclude() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/a.rs", "d/b.txt", "d/c.exe"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            ext: Some(AttrRuleSpec {
                include: OneOrMany::default(),
                exclude: OneOrMany::one(TextPatternSpec::exact("exe".to_string())),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/a.rs", "d/b.txt"]);
}

#[test]
fn locate_where_ext_exclude_wins_over_include() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/a.rs", "d/b.txt"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            ext: Some(AttrRuleSpec {
                include: OneOrMany::new(vec![
                    TextPatternSpec::exact("rs".to_string()),
                    TextPatternSpec::exact("txt".to_string()),
                ]),
                exclude: OneOrMany::one(TextPatternSpec::exact("txt".to_string())),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/a.rs"]);
}

#[test]
fn locate_where_attributes_combine_with_and_semantics() {
    let dir = TempDir::new().unwrap();
    create_test_files(
        &dir,
        &[
            "d/src/main.rs",
            "d/src/main.txt",
            "d/src/lib.rs",
            "d/tests/main.rs",
        ],
    );
    let include = |pattern: TextPatternSpec| AttrRuleSpec {
        include: OneOrMany::one(pattern),
        exclude: OneOrMany::default(),
    };
    let mut spec = source_with_root("d");
    spec.where_ = Some(WhereSpec {
        case: CaseMode::Sensitive,
        path: Some(include(TextPatternSpec::auto_detect("src/*".to_string()))),
        name: Some(include(TextPatternSpec::auto_detect("main.*".to_string()))),
        stem: Some(include(TextPatternSpec::exact("main".to_string()))),
        ext: Some(include(TextPatternSpec::exact("rs".to_string()))),
    });

    let result = spec.locate(Some(dir.path())).unwrap();

    assert_eq!(relative_path_strings(&dir, &result), ["d/src/main.rs"]);
}

// ---------------------------------------------------------------------------------------------- //
// Name and Stem Filters

#[test]
fn locate_where_name_include() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/foo.txt", "d/bar.txt", "d/baz.rs"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            name: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::exact("foo.txt".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/foo.txt"]);
}

#[test]
fn locate_where_stem_include() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/readme.txt", "d/readme.md", "d/other.txt"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            stem: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::exact("readme".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/readme.md", "d/readme.txt"]);
}

#[test]
fn locate_where_name_wildcard() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/test_foo.rs", "d/test_bar.rs", "d/main.rs"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            name: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec {
                    pattern: "test_*".to_string(),
                    syntax: PatternSyntax::Wildcard,
                }),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/test_bar.rs", "d/test_foo.rs"]);
}

#[test]
fn locate_where_case_sensitive() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/video.MP4", "d/audio.mp4"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            case: CaseMode::Sensitive,
            ext: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::exact("mp4".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/audio.mp4"]);
}

#[test]
fn prune_wildcard_braces_are_literal() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["a.rs", "b.toml", "c.txt"]);

    // Prune pattern with braces should NOT expand as alternation.
    // If braces were expanded, "*.{rs,toml}" would prune a.rs and b.toml.
    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: ".".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::one(PathPatternSpec {
                path: "*.{rs,toml}".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: true,
            }),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["a.rs", "b.toml", "c.txt"]);
}

// ---------------------------------------------------------------------------------------------- //
// Literal Where-Pattern Syntax

#[test]
fn locate_where_wildcard_bracket_syntax_is_literal() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["match.[invalid", "[invalid-name.txt", "other.rs"]);

    let mut by_extension = source_with_root(".");
    by_extension.where_ = Some(WhereSpec {
        ext: Some(AttrRuleSpec {
            include: OneOrMany::one(TextPatternSpec {
                pattern: "[invalid".to_string(),
                syntax: PatternSyntax::Wildcard,
            }),
            exclude: OneOrMany::default(),
        }),
        ..Default::default()
    });
    let extension_result = by_extension.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &extension_result), ["match.[invalid"]);

    let mut by_name = source_with_root(".");
    by_name.where_ = Some(WhereSpec {
        name: Some(AttrRuleSpec {
            include: OneOrMany::one(TextPatternSpec {
                pattern: "[invalid*".to_string(),
                syntax: PatternSyntax::Wildcard,
            }),
            exclude: OneOrMany::default(),
        }),
        ..Default::default()
    });
    let name_result = by_name.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &name_result), ["[invalid-name.txt"]);
}

// ---------------------------------------------------------------------------------------------- //
// Path Filters

#[test]
fn locate_where_path_exact_relative_include() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/a.txt", "d/sub/b.txt", "d/sub/c.txt"]);

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            path: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::exact("sub/b.txt".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/sub/b.txt"]);
}

#[test]
fn locate_where_path_wildcard_relative_exclude() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/keep.rs", "d/remove.tmp", "d/sub/also.tmp"]);

    let spec = source_with_root_and_path_exclude("d", "**/*.tmp");
    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/keep.rs"]);
}

#[test]
fn locate_where_path_wildcard_absolute_exclude() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/keep.rs", "d/skip.tmp"]);

    let abs_pattern = format!("{}/**/*.tmp", dir.path().display());
    let spec = source_with_root_and_path_exclude("d", &abs_pattern);
    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/keep.rs"]);
}

#[test]
fn locate_where_path_relative_include_matches_each_source_root() {
    let dir = TempDir::new().unwrap();
    create_test_files(
        &dir,
        &[
            "project-a/src/main.rs",
            "project-a/readme.md",
            "project-b/src/main.rs",
            "project-b/readme.md",
        ],
    );

    let source = |root: &str| SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: root.to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            path: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::auto_detect("src/*.rs".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let files = Files {
        sources: vec![source("project-a"), source("project-b")],
    };
    let result = files.locate(Some(dir.path())).unwrap();
    assert_eq!(
        relative_path_strings(&dir, &result),
        ["project-a/src/main.rs", "project-b/src/main.rs"]
    );
}

#[test]
fn locate_where_path_wildcard_root_uses_each_matched_dir_as_base() {
    let dir = TempDir::new().unwrap();
    create_test_files(
        &dir,
        &[
            "2020/img/a.png",
            "2020/thumbs/a.png",
            "2021/img/b.png",
            "2021/thumbs/b.png",
        ],
    );

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "202*".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: Some(WhereSpec {
            path: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::auto_detect("img/*.png".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["2020/img/a.png", "2021/img/b.png"]);
}

#[test]
fn locate_where_path_combined_with_prune() {
    let dir = TempDir::new().unwrap();
    create_test_files(
        &dir,
        &[
            "d/keep.rs",
            "d/remove.tmp",
            "d/target/pruned.rs",
            "d/target/deep/pruned.txt",
        ],
    );

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::one(PathPatternSpec {
                path: "d/target".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
        }),
        where_: Some(WhereSpec {
            path: Some(AttrRuleSpec {
                include: OneOrMany::default(),
                exclude: OneOrMany::one(TextPatternSpec::auto_detect("**/*.tmp".to_string())),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/keep.rs"]);
}

// ---------------------------------------------------------------------------------------------- //
// Files-Specific Ordering Integration

#[test]
fn locate_preserves_root_batches_sorting_and_first_seen_deduplication() {
    let dir = TempDir::new().unwrap();
    create_test_files(
        &dir,
        &[
            "first/item-3.txt",
            "first/item-1.txt",
            "second/item-10.txt",
            "second/item-2.txt",
        ],
    );

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::new(vec![
                exact_root("second"),
                exact_root("second/item-2.txt"),
                exact_root("first"),
            ]),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(
        relative_path_strings(&dir, &result),
        [
            "second/item-2.txt",
            "second/item-10.txt",
            "first/item-1.txt",
            "first/item-3.txt",
        ]
    );
}

#[test]
fn locate_exact_directory_uses_selected_path_order() {
    let dir = TempDir::new().unwrap();
    create_test_files(
        &dir,
        &[
            "d/file-11.txt",
            "d/file-2.txt",
            "d/file-10.txt",
            "d/file-1.txt",
        ],
    );

    let mut natural = source_with_root("d");
    natural.sort = PathSort::Natural;
    let natural_result = natural.locate(Some(dir.path())).unwrap();
    assert_eq!(
        relative_path_strings(&dir, &natural_result),
        [
            "d/file-1.txt",
            "d/file-2.txt",
            "d/file-10.txt",
            "d/file-11.txt"
        ]
    );

    let mut lexicographic = source_with_root("d");
    lexicographic.sort = PathSort::Lexicographic;
    let lexicographic_result = lexicographic.locate(Some(dir.path())).unwrap();
    assert_eq!(
        relative_path_strings(&dir, &lexicographic_result),
        [
            "d/file-1.txt",
            "d/file-10.txt",
            "d/file-11.txt",
            "d/file-2.txt"
        ]
    );
}

#[test]
fn locate_exact_directory_naturally_orders_directory_components() {
    let dir = TempDir::new().unwrap();
    create_test_files(
        &dir,
        &[
            "d/part-10/item.txt",
            "d/part-2/item.txt",
            "d/part-1/item.txt",
        ],
    );

    let result = source_with_root("d").locate(Some(dir.path())).unwrap();

    assert_eq!(
        relative_path_strings(&dir, &result),
        [
            "d/part-1/item.txt",
            "d/part-2/item.txt",
            "d/part-10/item.txt"
        ]
    );
}

#[test]
fn locate_wildcard_uses_selected_path_order() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["item-10.txt", "item-2.txt", "item-1.txt"]);

    let source = |sort| SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "item-*.txt".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort,
    };

    let natural = source(PathSort::Natural).locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &natural), ["item-1.txt", "item-2.txt", "item-10.txt"]);

    let lexicographic = source(PathSort::Lexicographic).locate(Some(dir.path())).unwrap();
    assert_eq!(
        relative_path_strings(&dir, &lexicographic),
        ["item-1.txt", "item-10.txt", "item-2.txt"]
    );
}

#[test]
fn locate_wildcard_naturally_orders_matched_directories() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["set-10/item.txt", "set-2/item.txt", "set-1/item.txt"]);
    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "set-*".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();

    assert_eq!(
        relative_path_strings(&dir, &result),
        ["set-1/item.txt", "set-2/item.txt", "set-10/item.txt"]
    );
}

#[test]
fn locate_filter_and_postcollection_prune_preserve_natural_order() {
    let dir = TempDir::new().unwrap();
    create_test_files(
        &dir,
        &[
            "d/item-10.txt",
            "d/item-3.tmp",
            "d/item-2.txt",
            "d/item-1.txt",
        ],
    );
    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::one(PathPatternSpec {
                path: "d/item-2.*".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: true,
            }),
        }),
        where_: Some(WhereSpec {
            ext: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::exact("txt".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        }),
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();

    assert_eq!(relative_path_strings(&dir, &result), ["d/item-1.txt", "d/item-10.txt"]);
}

#[test]
fn files_string_shorthand_uses_natural_order() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["item-10.txt", "item-2.txt", "item-1.txt"]);
    let path = dir.path().to_str().unwrap().replace('\\', "\\\\");
    let node = Node::parse_str(&format!(r#""{path}""#), Format::Json).unwrap();
    let files: Files = node.as_type().unwrap();

    let result = files.locate(None).unwrap();

    assert_eq!(relative_path_strings(&dir, &result), ["item-1.txt", "item-2.txt", "item-10.txt"]);
}
// ---------------------------------------------------------------------------------------------- //
// Unicode and Error Policy

#[cfg(any(unix, windows))]
#[test]
fn require_unicode_path_retains_the_exact_opaque_path() {
    let path = PathBuf::from(opaque_component(b'v'));

    assert_non_unicode_error(require_unicode_path(&path), &path);
}

#[cfg(any(unix, windows))]
#[test]
fn used_opaque_bases_are_structural_errors_for_every_policy() {
    let opaque_base = PathBuf::from(opaque_component(b'b'));
    let relative_exact = source_with_root("file.txt");
    let relative_wildcard = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "*.txt".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };
    let implicit = SourceSpec::default();
    let empty_root = SourceSpec {
        from: Some(FromSpec::default()),
        ..Default::default()
    };

    for on_error in [OnError::Fail, OnError::Warn, OnError::Ignore] {
        for spec in [&relative_exact, &relative_wildcard, &implicit, &empty_root] {
            assert_non_unicode_error(spec.locate_with(Some(&opaque_base), on_error), &opaque_base);
        }
    }
}

#[cfg(any(unix, windows))]
#[test]
fn relative_exact_prune_makes_an_opaque_base_structural() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["root/file.txt"]);
    let opaque_base = PathBuf::from(opaque_component(b'p'));
    let root = dir.path().join("root");
    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: root.to_str().unwrap().to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::one(PathPatternSpec {
                path: "optional-prune".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: false,
                recursive: true,
            }),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    for on_error in [OnError::Fail, OnError::Warn, OnError::Ignore] {
        assert_non_unicode_error(spec.locate_with(Some(&opaque_base), on_error), &opaque_base);
    }
}

#[cfg(any(unix, windows))]
#[test]
fn relative_wildcard_prune_makes_an_opaque_base_structural() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["root/file.txt"]);
    let opaque_base = PathBuf::from(opaque_component(b'g'));
    let root = dir.path().join("root");
    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: root.to_str().unwrap().to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::one(PathPatternSpec {
                path: "**/*.tmp".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: true,
            }),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    for on_error in [OnError::Fail, OnError::Warn, OnError::Ignore] {
        assert_non_unicode_error(spec.locate_with(Some(&opaque_base), on_error), &opaque_base);
    }
}

#[cfg(any(unix, windows))]
#[test]
fn unused_opaque_base_does_not_affect_an_absolute_source() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["root/file.txt"]);
    let opaque_base = PathBuf::from(opaque_component(b'u'));
    let root = dir.path().join("root");
    let spec = source_with_root(root.to_str().unwrap());

    let result = spec.locate_with(Some(&opaque_base), OnError::Fail).unwrap();

    assert_eq!(result, [root.join("file.txt")]);
}
#[cfg(any(unix, windows))]
#[test]
fn locate_opaque_file_follows_error_policy_before_sorting() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/file-10.txt", "d/file-2.txt"]);
    let opaque = create_opaque_file(&dir.path().join("d"), b'f');

    for sort in [PathSort::Natural, PathSort::Lexicographic] {
        let mut spec = source_with_root("d");
        spec.sort = sort;

        assert_non_unicode_error(spec.locate_with(Some(dir.path()), OnError::Fail), &opaque);

        for on_error in [OnError::Warn, OnError::Ignore] {
            let result = spec.locate_with(Some(dir.path()), on_error).unwrap();
            let expected = match sort {
                PathSort::Natural => ["d/file-2.txt", "d/file-10.txt"],
                PathSort::Lexicographic => ["d/file-10.txt", "d/file-2.txt"],
            };
            assert_eq!(relative_path_strings(&dir, &result), expected);
            assert!(result.iter().all(|path| path.to_str().is_some()));
        }
    }
}

#[cfg(any(unix, windows))]
#[test]
fn locate_opaque_entry_is_rejected_before_where_filters() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/keep.rs"]);
    let opaque = create_opaque_file(&dir.path().join("d"), b'w');

    let filters = [
        WhereSpec {
            path: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::exact("never".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        },
        WhereSpec {
            name: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::exact("never".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        },
        WhereSpec {
            stem: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::exact("never".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        },
        WhereSpec {
            ext: Some(AttrRuleSpec {
                include: OneOrMany::one(TextPatternSpec::exact("txt".to_string())),
                exclude: OneOrMany::default(),
            }),
            ..Default::default()
        },
    ];

    for where_ in filters {
        let mut spec = source_with_root("d");
        spec.where_ = Some(where_);
        assert_non_unicode_error(spec.locate_with(Some(dir.path()), OnError::Fail), &opaque);
    }
}

#[cfg(any(unix, windows))]
#[test]
fn locate_opaque_entry_is_rejected_before_postcollection_pruning() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("d")).unwrap();
    let opaque = create_opaque_file(&dir.path().join("d"), b'p');
    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: true,
            }),
            prune: OneOrMany::one(PathPatternSpec {
                path: "**/*.txt".to_string(),
                syntax: PatternSyntax::Wildcard,
                must_exist: false,
                recursive: true,
            }),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let compiled =
        CompiledPrune::build(Some(dir.path()), &spec.from.as_ref().unwrap().prune).unwrap();
    assert!(compiled.is_excluded(&opaque, Some(dir.path())));

    assert_non_unicode_error(spec.locate_with(Some(dir.path()), OnError::Fail), &opaque);
}

#[cfg(any(unix, windows))]
#[test]
fn locate_opaque_directory_follows_policy_and_omits_its_subtree() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/keep.txt"]);
    let opaque = create_opaque_dir(&dir.path().join("d"), b'd');
    fs::write(opaque.join("hidden.txt"), "hidden beneath opaque directory").unwrap();

    let spec = source_with_root("d");
    assert_non_unicode_error(spec.locate_with(Some(dir.path()), OnError::Fail), &opaque);

    let result = spec.locate_with(Some(dir.path()), OnError::Ignore).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/keep.txt"]);

    let non_recursive = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "d".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: true,
                recursive: false,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };
    assert_non_unicode_error(non_recursive.locate_with(Some(dir.path()), OnError::Fail), &opaque);
}
// ---------------------------------------------------------------------------------------------- //
// Symlinks and Special Path Types

#[cfg(unix)]
#[test]
fn locate_dir_includes_symlinked_file() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/real.txt"]);

    std::os::unix::fs::symlink(dir.path().join("d/real.txt"), dir.path().join("d/link.txt"))
        .unwrap();

    let spec = source_with_root("d");

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/link.txt", "d/real.txt"]);
}

#[cfg(unix)]
#[test]
fn locate_dir_does_not_recurse_symlinked_dir() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/a.txt", "other/b.txt"]);

    std::os::unix::fs::symlink(dir.path().join("other"), dir.path().join("d/linked")).unwrap();

    let spec = source_with_root("d");

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/a.txt"]);
}

#[cfg(unix)]
#[test]
fn locate_dir_skips_broken_symlink() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/a.txt"]);

    std::os::unix::fs::symlink("/nonexistent/target", dir.path().join("d/broken.txt")).unwrap();

    let spec = source_with_root("d");

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/a.txt"]);
}

#[cfg(unix)]
#[test]
fn locate_root_symlink_to_dir_errors() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["real_dir/a.txt"]);

    std::os::unix::fs::symlink(dir.path().join("real_dir"), dir.path().join("link_to_dir"))
        .unwrap();

    let spec = source_with_root("link_to_dir");
    let expected = dir.path().join("link_to_dir");

    assert!(matches!(
        spec.locate(Some(dir.path())),
        Err(FilesError::SymlinkDirNotTraversable { path }) if path == expected
    ));
}

#[cfg(unix)]
#[test]
fn locate_wildcard_symlink_to_directory_errors() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["real_dir/a.txt"]);
    let link = dir.path().join("link-dir");
    std::os::unix::fs::symlink(dir.path().join("real_dir"), &link).unwrap();

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(wildcard_root("link-*")),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    assert!(matches!(
        spec.locate(Some(dir.path())),
        Err(FilesError::SymlinkDirNotTraversable { path }) if path == link
    ));
}

#[cfg(unix)]
#[test]
fn locate_root_symlink_to_file_works() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["real.txt"]);

    std::os::unix::fs::symlink(dir.path().join("real.txt"), dir.path().join("link.txt")).unwrap();

    let spec = source_with_root("link.txt");

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["link.txt"]);
}

#[cfg(unix)]
#[test]
fn locate_root_broken_symlink_must_exist_errors() {
    let dir = TempDir::new().unwrap();

    std::os::unix::fs::symlink("/nonexistent/target", dir.path().join("broken")).unwrap();

    let spec = source_with_root("broken");
    let expected = dir.path().join("broken");

    assert!(matches!(
        spec.locate(Some(dir.path())),
        Err(FilesError::RootPathNotFound { path }) if path == expected
    ));
}

#[cfg(unix)]
#[test]
fn locate_root_broken_symlink_not_must_exist_ok() {
    let dir = TempDir::new().unwrap();

    std::os::unix::fs::symlink("/nonexistent/target", dir.path().join("broken")).unwrap();

    let spec = SourceSpec {
        from: Some(FromSpec {
            root: OneOrMany::one(PathPatternSpec {
                path: "broken".to_string(),
                syntax: PatternSyntax::Exact,
                must_exist: false,
                recursive: true,
            }),
            prune: OneOrMany::default(),
        }),
        where_: None,
        sort: PathSort::Natural,
    };

    let result = spec.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), Vec::<String>::new());
}

// ---------------------------------------------------------------------------------------------- //
// Multi-Source Aggregation

#[test]
fn files_locate_empty_sources() {
    let files = Files::default();
    let result = files.locate(Some(Path::new("/tmp"))).unwrap();
    assert!(result.is_empty());
}

#[test]
fn files_locate_aggregates_sources_in_order() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["src/lib.rs", "src/main.rs", "docs/readme.md"]);

    let files = Files {
        sources: vec![
            SourceSpec {
                from: Some(FromSpec {
                    root: OneOrMany::one(PathPatternSpec {
                        path: "src".to_string(),
                        syntax: PatternSyntax::Exact,
                        must_exist: true,
                        recursive: true,
                    }),
                    prune: OneOrMany::default(),
                }),
                where_: Some(WhereSpec {
                    ext: Some(AttrRuleSpec {
                        include: OneOrMany::one(TextPatternSpec::exact("rs".to_string())),
                        exclude: OneOrMany::default(),
                    }),
                    ..Default::default()
                }),
                sort: PathSort::Natural,
            },
            SourceSpec {
                from: Some(FromSpec {
                    root: OneOrMany::one(PathPatternSpec {
                        path: "docs".to_string(),
                        syntax: PatternSyntax::Exact,
                        must_exist: true,
                        recursive: true,
                    }),
                    prune: OneOrMany::default(),
                }),
                where_: Some(WhereSpec {
                    ext: Some(AttrRuleSpec {
                        include: OneOrMany::one(TextPatternSpec::exact("md".to_string())),
                        exclude: OneOrMany::default(),
                    }),
                    ..Default::default()
                }),
                sort: PathSort::Natural,
            },
        ],
    };

    let result = files.locate(Some(dir.path())).unwrap();
    assert_eq!(
        relative_path_strings(&dir, &result),
        ["src/lib.rs", "src/main.rs", "docs/readme.md"]
    );
}

#[test]
fn files_locate_deduplication_across_sources() {
    let dir = TempDir::new().unwrap();
    create_test_files(&dir, &["d/a.txt", "d/b.txt"]);

    let files = Files {
        sources: vec![source_with_root("d"), source_with_root("d")],
    };

    let result = files.locate(Some(dir.path())).unwrap();
    assert_eq!(relative_path_strings(&dir, &result), ["d/a.txt", "d/b.txt"]);
}

#[test]
fn files_locate_keeps_source_groups_with_independent_sort_modes() {
    let dir = TempDir::new().unwrap();
    create_test_files(
        &dir,
        &[
            "first/item-10.txt",
            "first/item-2.txt",
            "second/item-10.txt",
            "second/item-2.txt",
        ],
    );
    let mut first = source_with_root("first");
    first.sort = PathSort::Lexicographic;
    let mut second = source_with_root("second");
    second.sort = PathSort::Natural;
    let files = Files {
        sources: vec![first, second],
    };

    let result = files.locate(Some(dir.path())).unwrap();

    assert_eq!(
        relative_path_strings(&dir, &result),
        [
            "first/item-10.txt",
            "first/item-2.txt",
            "second/item-2.txt",
            "second/item-10.txt",
        ]
    );
}
