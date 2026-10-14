use super::*;
use std::io::{self, ErrorKind};

// ---------------------------------------------------------------------------------------------- //

#[test]
fn successful_results_keep_the_original_value() {
    let value = vec!["source", "destination"];
    assert_eq!(Ok(value).not_found_ok().unwrap(), Some(vec!["source", "destination"]));
    assert_eq!(Ok(()).not_found_ok().unwrap(), Some(()));
}

#[test]
fn only_not_found_becomes_absence() {
    let missing = io::Error::new(ErrorKind::NotFound, "missing target");
    assert!(Err::<(), _>(missing).not_found_ok().unwrap().is_none());

    let error = io::Error::new(ErrorKind::PermissionDenied, FailureContext("source is protected"));
    let context = error.get_ref().unwrap() as *const _;
    let returned = Err::<(), _>(error).not_found_ok().unwrap_err();
    assert_eq!(returned.kind(), ErrorKind::PermissionDenied);
    assert!(std::ptr::eq(context, returned.get_ref().unwrap()));
    assert_eq!(
        returned.get_ref().unwrap().downcast_ref::<FailureContext>().unwrap().0,
        "source is protected"
    );

    let error = io::Error::from_raw_os_error(5);
    let original_kind = error.kind();
    let returned = Err::<(), _>(error).not_found_ok().unwrap_err();
    assert_eq!(returned.kind(), original_kind);
    assert_eq!(returned.raw_os_error(), Some(5));
}

#[test]
fn fs_err_diagnostics_survive_optional_handling() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing.txt");
    assert!(fs_err::read_to_string(&missing).not_found_ok().unwrap().is_none());

    let error = fs_err::read_to_string(temp.path()).unwrap_err();
    assert_ne!(error.kind(), ErrorKind::NotFound);
    let original_message = error.to_string();
    let original_kind = error.kind();
    let context = error.get_ref().unwrap() as *const _;
    let returned = Err::<String, _>(error).not_found_ok().unwrap_err();
    assert_eq!(returned.kind(), original_kind);
    assert_eq!(returned.to_string(), original_message);
    assert!(std::ptr::eq(context, returned.get_ref().unwrap()));
    assert!(returned.to_string().contains(&temp.path().display().to_string()));
}

#[cfg(any(unix, windows))]
#[test]
fn optional_metadata_retains_the_callers_final_link_policy() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("missing-target.txt");
    let link = temp.path().join("dangling-link.txt");
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(&target, &link);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_file(&target, &link);
    match result {
        Ok(()) => {}
        Err(error)
            if matches!(error.kind(), ErrorKind::PermissionDenied | ErrorKind::Unsupported) =>
        {
            eprintln!("skipping dangling final-link coverage: {error}");
            return;
        }
        Err(error) => panic!("create temporary dangling link: {error}"),
    }

    assert!(fs_err::metadata(&link).not_found_ok().unwrap().is_none());
    let entry = fs_err::symlink_metadata(&link).not_found_ok().unwrap().unwrap();
    assert!(entry.file_type().is_symlink());

    use crate::util::PathExt;
    assert_eq!(link.require_missing().unwrap(), link);
    assert!(matches!(link.require_file(), Err(crate::util::path_ext::PathError::NotFound { .. })));
}

#[derive(Debug, thiserror::Error)]
#[error("I/O context: {0}")]
struct FailureContext(&'static str);
