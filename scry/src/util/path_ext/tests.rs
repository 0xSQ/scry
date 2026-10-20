use super::*;
use std::fs;

// ---------------------------------------------------------------------------------------------- //

#[test]
fn ext_str_missing_gives_missing_component_not_invalid_utf8() {
    let p = Path::new("no_extension");
    match p.ext_str() {
        Err(PathError::MissingComponent { component, .. }) => {
            assert_eq!(component, "file extension");
        }
        other => panic!("expected MissingComponent, got: {other:?}"),
    }
}

#[test]
fn file_name_str_missing_gives_missing_component() {
    let p = Path::new("");
    match p.file_name_str() {
        Err(PathError::MissingComponent { component, .. }) => {
            assert_eq!(component, "file name");
        }
        other => panic!("expected MissingComponent, got: {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn ext_str_invalid_utf8_on_unix() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let mut bytes = b"foo.".to_vec();
    bytes.push(0xFF);
    let os = OsString::from_vec(bytes);
    let p = PathBuf::from(os);

    match p.ext_str() {
        Err(PathError::InvalidUtf8 { component, .. }) => {
            assert_eq!(component, "file extension");
        }
        other => panic!("expected InvalidUtf8, got: {other:?}"),
    }
}

#[test]
fn ensure_dir_errors_if_path_is_file() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();

    let file_path = base.join("not_a_dir");
    fs::write(&file_path, b"hi").unwrap();

    let res = file_path.ensure_dir();
    assert!(matches!(res, Err(PathError::NotADir { .. })));
}

#[test]
fn ensure_dir_all_errors_if_path_is_file() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();

    let file_path = base.join("not_a_dir");
    fs::write(&file_path, b"hi").unwrap();

    let res = file_path.ensure_dir_all();
    assert!(matches!(res, Err(PathError::NotADir { .. })));
}

#[test]
fn ensure_dir_errors_if_parent_missing() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();

    let p = base.join("missing_parent").join("child");
    let res = p.ensure_dir();

    match res {
        Err(PathError::ParentNotFound { parent, .. }) => {
            assert_eq!(parent, base.join("missing_parent"));
        }
        other => panic!("expected ParentNotFound, got: {other:?}"),
    }
}

#[test]
fn ensure_dir_all_creates_parents() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();

    let p = base.join("a").join("b").join("c");
    p.ensure_dir_all().unwrap();

    assert!(p.is_dir());
}

#[test]
fn recreate_dir_errors_if_path_is_file() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();

    let file_path = base.join("not_a_dir");
    fs::write(&file_path, b"hi").unwrap();

    let res = file_path.recreate_dir();
    assert!(matches!(res, Err(PathError::NotADir { .. })));
}

#[test]
fn recreate_dir_all_removes_existing_contents() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();

    let dir = base.join("x").join("y");
    dir.ensure_dir_all().unwrap();

    let file = dir.join("old.txt");
    fs::write(&file, "hello").unwrap();
    assert!(file.exists());

    dir.recreate_dir_all().unwrap();
    assert!(dir.is_dir());
    assert!(!file.exists());
}

#[cfg(any(unix, windows))]
#[test]
fn recreation_replaces_final_directory_links_without_removing_target_contents() {
    let temp = tempfile::tempdir().unwrap();
    for recursive in [false, true] {
        let target = temp.path().join(format!("target-{recursive}"));
        let link = temp.path().join(format!("link-{recursive}"));
        fs::create_dir(&target).unwrap();
        let sentinel = target.join("keep.txt");
        fs::write(&sentinel, "preserve linked contents").unwrap();
        if !directory_alias(&target, &link) {
            eprintln!("skipping final-directory-link recreation coverage");
            return;
        }
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());

        let result = if recursive {
            link.recreate_dir_all()
        } else {
            link.recreate_dir()
        };
        assert_eq!(result.unwrap(), link);
        let entry = fs::symlink_metadata(&link).unwrap();
        assert!(entry.is_dir());
        assert!(!entry.file_type().is_symlink());
        assert_eq!(fs::read_dir(&link).unwrap().count(), 0);
        assert_eq!(fs::read_to_string(&sentinel).unwrap(), "preserve linked contents");
    }
}

#[cfg(any(unix, windows))]
#[test]
fn recreation_preserves_dangling_final_directory_links_on_failure() {
    let temp = tempfile::tempdir().unwrap();
    for recursive in [false, true] {
        let target = temp.path().join(format!("missing-{recursive}"));
        let link = temp.path().join(format!("link-{recursive}"));
        if !directory_alias(&target, &link) {
            eprintln!("skipping dangling-directory-link recreation coverage");
            return;
        }
        let original_target = fs::read_link(&link).unwrap();

        let result = if recursive {
            link.recreate_dir_all()
        } else {
            link.recreate_dir()
        };
        assert!(result.is_err(), "recreation unexpectedly replaced a dangling link");
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_link(&link).unwrap(), original_target);
        assert!(!target.try_exists().unwrap());
    }
}

#[test]
fn has_ext_is_case_insensitive() {
    let p = Path::new("config.RHAI");
    assert!(p.has_ext("rhai"));
    assert!(p.has_ext("RHAI"));
    assert!(!p.has_ext("toml"));
}

#[test]
fn ensure_dir_errors_if_parent_is_file() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();

    // Create a file where the "parent" should be
    let fake_parent = base.join("not_a_dir");
    fs::write(&fake_parent, b"hi").unwrap();

    // Try to create a child directory under the file
    let child = fake_parent.join("child");
    let res = child.ensure_dir();

    match res {
        Err(PathError::NotADir { path }) => {
            assert_eq!(path, fake_parent);
        }
        other => panic!("expected NotADir for parent, got: {other:?}"),
    }
}

#[test]
fn recreate_dir_errors_if_parent_is_file() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();

    // Create a file where the "parent" should be
    let fake_parent = base.join("not_a_dir");
    fs::write(&fake_parent, b"hi").unwrap();

    // Try to recreate a child directory under the file
    let child = fake_parent.join("child");
    let res = child.recreate_dir();

    match res {
        Err(PathError::NotADir { path }) => {
            assert_eq!(path, fake_parent);
        }
        other => panic!("expected NotADir for parent, got: {other:?}"),
    }
}

// --- require_parent_dir() tests ---

#[test]
fn require_parent_dir_ok_for_cwd_relative_file() {
    // No filesystem assumptions needed: parent is "" => no-op
    let p = Path::new("file.txt");
    p.require_parent_dir().unwrap();
}

#[test]
fn require_parent_dir_errors_if_parent_missing() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();

    let p = base.join("missing_parent").join("child.txt");
    let res = p.require_parent_dir();

    match res {
        Err(PathError::ParentNotFound { parent, .. }) => {
            assert_eq!(parent, base.join("missing_parent"));
        }
        other => panic!("expected ParentNotFound, got: {other:?}"),
    }
}

#[test]
fn require_parent_dir_errors_if_parent_is_file() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();

    let fake_parent = base.join("not_a_dir");
    fs::write(&fake_parent, b"hi").unwrap();

    let child = fake_parent.join("child.txt");
    let res = child.require_parent_dir();

    match res {
        Err(PathError::NotADir { path }) => {
            assert_eq!(path, fake_parent);
        }
        other => panic!("expected NotADir for parent, got: {other:?}"),
    }
}

#[test]
fn ends_with_suffixes_matches_compound() {
    let p = Path::new("archive.tar.gz");
    assert!(p.ends_with_suffixes(&["tar", "gz"]));
    assert!(p.ends_with_suffixes(&["gz"]));
    assert!(!p.ends_with_suffixes(&["zip"]));
    assert!(!p.ends_with_suffixes(&["tar", "zip"]));
}

#[test]
fn ends_with_suffixes_case_insensitive() {
    let p = Path::new("ARCHIVE.TAR.GZ");
    assert!(p.ends_with_suffixes(&["tar", "gz"]));
    assert!(p.ends_with_suffixes(&["TAR", "GZ"]));
}

#[test]
fn ends_with_suffixes_empty_returns_false() {
    let p = Path::new("archive.tar.gz");
    assert!(!p.ends_with_suffixes(&[]));
}

#[test]
fn ends_with_suffixes_dotfile_not_matched_as_suffix() {
    // ".bashrc" should NOT match ["bashrc"] because that would consume the leading dot
    let p = Path::new(".bashrc");
    assert!(!p.ends_with_suffixes(&["bashrc"]));
}

#[test]
fn ends_with_suffixes_dotfile_with_extension() {
    // ".config.json" should match ["json"]
    let p = Path::new(".config.json");
    assert!(p.ends_with_suffixes(&["json"]));
    // But ["config", "json"] would leave just "." as base, so no match
    assert!(!p.ends_with_suffixes(&["config", "json"]));
}

#[test]
fn strip_suffixes_compound() {
    let p = Path::new("my.archive.tar.gz");
    assert_eq!(p.strip_suffixes(&["tar", "gz"]), Some(PathBuf::from("my.archive")));
    assert_eq!(p.strip_suffixes(&["gz"]), Some(PathBuf::from("my.archive.tar")));
    assert_eq!(p.strip_suffixes(&["zip"]), None);
}

#[test]
fn strip_suffixes_preserves_parent_path() {
    let p = Path::new("/some/path/archive.tar.gz");
    assert_eq!(p.strip_suffixes(&["tar", "gz"]), Some(PathBuf::from("/some/path/archive")));
}

#[test]
fn strip_suffixes_dotfile() {
    // ".config.json" stripping ["json"] yields ".config"
    let p = Path::new(".config.json");
    assert_eq!(p.strip_suffixes(&["json"]), Some(PathBuf::from(".config")));
}

#[test]
fn strip_suffixes_empty_base_returns_none() {
    // Stripping all of ".tar.gz" from ".tar.gz" would leave empty base
    let p = Path::new(".tar.gz");
    assert_eq!(p.strip_suffixes(&["tar", "gz"]), None);
}

#[test]
fn strip_suffixes_dot_base_returns_none() {
    // "..json" stripping ["json"] would leave just "." which is invalid
    let p = Path::new("..json");
    assert!(!p.ends_with_suffixes(&["json"]));
    assert_eq!(p.strip_suffixes(&["json"]), None);
}
#[test]
fn strip_suffixes_does_not_produce_a_parent_component() {
    let p = Path::new("some/path/...json");
    assert!(!p.ends_with_suffixes(&["json"]));
    assert_eq!(p.strip_suffixes(&["json"]), None);
}

#[test]
fn lexical_components_are_borrowed_for_paths_and_path_buffers() {
    fn borrowed_components(path: &Path) -> (&OsStr, &OsStr, &Path) {
        (
            PathExt::require_file_name(path).unwrap(),
            PathExt::require_stem(path).unwrap(),
            PathExt::require_parent(path).unwrap(),
        )
    }

    let path = PathBuf::from("missing-directory/image.edited.png");
    let (name, stem, parent) = borrowed_components(path.as_path());
    assert_eq!(name, OsStr::new("image.edited.png"));
    assert_eq!(stem, OsStr::new("image.edited"));
    assert_eq!(parent, Path::new("missing-directory"));
    assert_eq!(path.require_file_name().unwrap(), name);
    assert_eq!(path.require_stem().unwrap(), stem);
    assert_eq!(path.require_parent().unwrap(), parent);

    let simplified: &Path = PathExt::simplify_path(path.as_path());
    assert_eq!(simplified, path);
    assert_eq!(Path::new("image.png").require_parent().unwrap(), Path::new(""));
}

#[test]
fn missing_native_components_retain_the_original_path() {
    let path = Path::new("");
    for (result, expected_component) in [
        (path.require_file_name().map(|_| ()), "file name"),
        (path.require_stem().map(|_| ()), "file stem"),
        (path.require_parent().map(|_| ()), "parent"),
    ] {
        match result.unwrap_err() {
            PathError::MissingComponent {
                path: input,
                component,
            } => {
                assert_eq!(input, path);
                assert_eq!(component, expected_component);
            }
            other => panic!("expected MissingComponent, got: {other:?}"),
        }
    }

    #[cfg(windows)]
    let root = Path::new(r"C:\");
    #[cfg(not(windows))]
    let root = Path::new("/");
    assert!(matches!(root.require_parent(), Err(PathError::MissingComponent { .. })));
    assert!(matches!(root.require_file_name(), Err(PathError::MissingComponent { .. })));
}

#[test]
fn optional_extension_distinguishes_missing_empty_and_present() {
    assert_eq!(Path::new("image").ext_str_opt().unwrap(), None);
    assert_eq!(Path::new(".image").ext_str_opt().unwrap(), None);
    assert_eq!(Path::new("image.").ext_str_opt().unwrap(), Some(""));
    assert_eq!(Path::new("image.PNG").ext_str_opt().unwrap(), Some("PNG"));
}

#[cfg(any(unix, windows))]
#[test]
fn native_components_preserve_non_text_names_and_stems() {
    let name = non_text_name();
    let path = PathBuf::from("missing-parent").join(&name);
    assert_eq!(path.require_file_name().unwrap(), name);
    assert_eq!(path.require_stem().unwrap(), name);
    assert!(matches!(path.file_name_str(), Err(PathError::InvalidUtf8 { .. })));
    assert!(matches!(path.stem_str(), Err(PathError::InvalidUtf8 { .. })));

    let mut filename = name.clone();
    filename.push(".png");
    let path = PathBuf::from(filename);
    assert_eq!(path.require_stem().unwrap(), name);
    assert_eq!(path.ext_str_opt().unwrap(), Some("png"));
}

#[cfg(any(unix, windows))]
#[test]
fn optional_extension_rejects_non_text_instead_of_falling_back() {
    let mut name = std::ffi::OsString::from("image.");
    name.push(non_text_name());
    let path = PathBuf::from(name);
    match path.ext_str_opt().unwrap_err() {
        PathError::InvalidUtf8 {
            path: input,
            component,
        } => {
            assert_eq!(input, path);
            assert_eq!(component, "file extension");
        }
        other => panic!("expected InvalidUtf8, got: {other:?}"),
    }
}

#[test]
fn filesystem_validation_preserves_paths_and_distinguishes_types_and_absence() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("source.txt");
    let directory = temp.path().join("directory");
    let missing = temp.path().join("missing");
    fs::write(&file, "source").unwrap();
    fs::create_dir(&directory).unwrap();

    assert_eq!(file.require_exists().unwrap(), file);
    assert_eq!(file.require_file().unwrap(), file);
    assert_eq!(directory.require_dir().unwrap(), directory);
    assert_eq!(missing.require_missing().unwrap(), missing);
    assert!(
        matches!(file.require_missing(), Err(PathError::AlreadyExists { path }) if path == file)
    );
    assert!(matches!(file.require_dir(), Err(PathError::NotADir { path }) if path == file));
    assert!(
        matches!(directory.require_file(), Err(PathError::NotAFile { path }) if path == directory)
    );
    assert!(matches!(missing.require_file(), Err(PathError::NotFound { path }) if path == missing));
    assert!(matches!(missing.require_dir(), Err(PathError::NotFound { path }) if path == missing));
}

#[test]
fn directory_creation_recovers_only_for_existing_directories() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("source.txt");
    let directory = temp.path().join("directory");
    fs::write(&file, "keep source").unwrap();
    fs::create_dir(&directory).unwrap();
    for recursive in [false, true] {
        assert_eq!(create_directory(&directory, recursive).unwrap(), directory);
        assert!(matches!(
            create_directory(&file, recursive),
            Err(PathError::NotADir { path }) if path == file
        ));
    }
    assert_eq!(fs::read_to_string(&file).unwrap(), "keep source");
}

#[test]
fn lookup_failures_preserve_the_operation_input_and_io_error() {
    let path = Path::new("invalid\0path");
    let metadata_error = fs::metadata(path).unwrap_err();
    for result in [
        path.require_file(),
        path.require_dir(),
        path.ensure_dir(),
        path.ensure_dir_all(),
        path.recreate_dir(),
        path.recreate_dir_all(),
    ] {
        assert_io_error(result.unwrap_err(), path, "inspect metadata", &metadata_error);
    }

    let exists_error = path.try_exists().unwrap_err();
    for result in [path.require_exists(), path.require_missing()] {
        assert_io_error(result.unwrap_err(), path, "check existence", &exists_error);
    }

    let child = path.join("child");
    assert_io_error(
        child.require_parent_dir().unwrap_err(),
        path,
        "inspect metadata",
        &metadata_error,
    );

    let canonicalize_error = path.canonicalize().unwrap_err();
    assert_io_error(
        path.canonicalize_path().unwrap_err(),
        path,
        "canonicalize path",
        &canonicalize_error,
    );
}

#[test]
fn canonicalization_resolves_existing_files_and_relative_paths() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("image.txt");
    fs::write(&file, "source").unwrap();
    let resolved = file.canonicalize_path().unwrap();
    assert!(resolved.is_absolute());
    assert_eq!(fs::read_to_string(&resolved).unwrap(), "source");
    assert_eq!(PathExt::canonicalize_path(file.as_path()).unwrap(), resolved);
    #[cfg(windows)]
    assert!(!resolved.as_os_str().as_encoded_bytes().starts_with(b"\\\\?\\"));
    #[cfg(not(windows))]
    assert_eq!(resolved, fs::canonicalize(&file).unwrap());

    let working_directory = Path::new(".").canonicalize_path().unwrap();
    assert!(working_directory.is_absolute());
    assert_eq!(working_directory, std::env::current_dir().unwrap().canonicalize_path().unwrap());

    let missing = temp.path().join("missing");
    let original_error = fs::canonicalize(&missing).unwrap_err();
    assert_io_error(
        missing.canonicalize_path().unwrap_err(),
        &missing,
        "canonicalize path",
        &original_error,
    );
}

#[cfg(windows)]
#[test]
fn simplification_keeps_complete_future_paths_with_verbatim_semantics() {
    let ancestor = Path::new(r"\\?\C:\missing-scry-path-ext-tests");
    assert_eq!(ancestor.simplify_path(), Path::new(r"C:\missing-scry-path-ext-tests"));
    for suffix in ["trailing.", "trailing ", "CON.txt"] {
        let complete = ancestor.join(suffix);
        assert_eq!(complete.simplify_path(), complete);
    }
    let literal_parent = Path::new(r"\\?\C:\missing-scry-path-ext-tests\parent\..\file.txt");
    assert_eq!(literal_parent.simplify_path(), literal_parent);

    let mut long_path = ancestor.to_path_buf();
    for _ in 0..4 {
        long_path.push("a".repeat(70));
    }
    assert_eq!(long_path.simplify_path(), long_path);

    let share = Path::new(r"\\?\UNC\server\share\missing.txt");
    assert_eq!(share.simplify_path(), share);
    let mut non_text = ancestor.to_path_buf();
    non_text.push(non_text_name());
    assert_eq!(non_text.simplify_path(), non_text);

    let temp = tempfile::tempdir().unwrap();
    let complete = fs::canonicalize(temp.path()).unwrap().join("missing").join("trailing.");
    assert_eq!(complete.simplify_path(), complete);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[cfg(not(windows))]
#[test]
fn simplification_preserves_unix_paths_and_windows_like_filename_text() {
    for text in [
        "missing/../image.png",
        r"\\?\C:\folder\image.png",
        r"\\?\UNC\server\share",
    ] {
        let path = Path::new(text);
        assert_eq!(path.simplify_path(), path);
        assert!(std::ptr::eq(path.simplify_path(), path));
    }
}

#[cfg(any(unix, windows))]
#[test]
fn canonicalization_identifies_a_directory_alias_without_overwriting_the_source() {
    let temp = tempfile::tempdir().unwrap();
    let source_dir = temp.path().join("sources");
    let alias_dir = temp.path().join("alias");
    fs::create_dir(&source_dir).unwrap();
    let source = source_dir.join("source.txt");
    fs::write(&source, "keep source").unwrap();
    if !directory_alias(&source_dir, &alias_dir) {
        return;
    }

    assert_eq!(
        source.canonicalize_path().unwrap(),
        alias_dir.join("source.txt").canonicalize_path().unwrap()
    );
    assert_eq!(fs::read_to_string(&source).unwrap(), "keep source");
}

// ---------------------------------------------------------------------------------------------- //
// Test Helpers

fn assert_io_error(
    error: PathError,
    input: &Path,
    expected_operation: &str,
    original: &std::io::Error,
) {
    use std::error::Error;

    let message = error.to_string();
    assert!(message.contains(expected_operation));
    assert!(message.contains(&input.display().to_string()));
    let io_source = error.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
    assert_eq!(io_source.kind(), original.kind());
    assert_eq!(io_source.raw_os_error(), original.raw_os_error());
    assert_eq!(io_source.to_string(), original.to_string());
    match error {
        PathError::Io {
            path,
            operation,
            source,
        } => {
            assert_eq!(path, input);
            assert_eq!(operation, expected_operation);
            assert_eq!(source.kind(), original.kind());
            assert_eq!(source.raw_os_error(), original.raw_os_error());
        }
        other => panic!("expected Io, got: {other:?}"),
    }
}

#[cfg(unix)]
fn non_text_name() -> std::ffi::OsString {
    use std::os::unix::ffi::OsStringExt;

    std::ffi::OsString::from_vec(vec![b'n', 0xff])
}

#[cfg(windows)]
fn non_text_name() -> std::ffi::OsString {
    use std::os::windows::ffi::OsStringExt;

    std::ffi::OsString::from_wide(&[b'n' as u16, 0xd800])
}

#[cfg(any(unix, windows))]
fn directory_alias(target: &Path, alias: &Path) -> bool {
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(target, alias);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir(target, alias);
    match result {
        Ok(()) => true,
        Err(error)
            if error.kind() == std::io::ErrorKind::Unsupported
                || (cfg!(windows) && error.raw_os_error() == Some(1314)) =>
        {
            eprintln!("skipping directory alias coverage: {error}");
            false
        }
        Err(error) => panic!("create temporary directory alias: {error}"),
    }
}
