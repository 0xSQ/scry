use scry::{Config, StringEnum};

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Clone, Copy, PartialEq, Eq, StringEnum)]
enum PlainMode {
    RowMajor,
    ColMajor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, StringEnum)]
#[scry(rename_all = "kebab-case")]
enum KebabMode {
    RowMajor,
    ColMajor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, StringEnum)]
enum RenamedMode {
    #[scry(rename = "fast")]
    Quick,
    Careful,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Config)]
#[scry(from_str)]
enum ConfigMode {
    Summary,
    Raw,
}

#[test]
fn string_enum_displays_canonical_variant_name() {
    assert_eq!(PlainMode::RowMajor.to_string(), "row_major");
    assert_eq!(KebabMode::RowMajor.to_string(), "row-major");
    assert_eq!(RenamedMode::Quick.to_string(), "fast");
}

#[test]
fn string_enum_parses_canonical_names() {
    assert_eq!("row_major".parse::<PlainMode>().unwrap(), PlainMode::RowMajor);
    assert_eq!("row-major".parse::<KebabMode>().unwrap(), KebabMode::RowMajor);
    assert_eq!("fast".parse::<RenamedMode>().unwrap(), RenamedMode::Quick);
}

#[test]
fn string_enum_parses_case_insensitive_aliases() {
    assert_eq!("ROW-MAJOR".parse::<PlainMode>().unwrap(), PlainMode::RowMajor);
    assert_eq!("row_major".parse::<KebabMode>().unwrap(), KebabMode::RowMajor);
}

#[test]
fn string_enum_reports_expected_values() {
    let err = "diagonal".parse::<PlainMode>().unwrap_err();
    let message = err.to_string();

    assert!(message.contains("PlainMode"));
    assert!(message.contains("diagonal"));
    assert!(message.contains("row_major, col_major"));
}

#[test]
fn config_from_str_marker_generates_display_and_from_str() {
    assert_eq!(ConfigMode::Summary.to_string(), "summary");
    assert_eq!("raw".parse::<ConfigMode>().unwrap(), ConfigMode::Raw);
}
