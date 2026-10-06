use super::*;

// ---------------------------------------------------------------------------------------------- //

#[test]
fn nullable_labels_preserve_shapes_without_implying_omission() {
    let cases = [
        (Desc::plain("u32"), "u32 | null"),
        (Desc::default(), "value | null"),
        (Desc::structure(vec![]), "struct | null"),
        (Desc::enumeration(vec![]), "enum | null"),
        (Desc::tuple(vec![]), "tuple | null"),
        (Desc::list(Desc::plain("u32")), "list[u32] | null"),
    ];
    for (desc, expected) in cases {
        assert!(!desc.nullable);
        let desc = desc.nullable().nullable();
        assert_eq!(desc.type_label(), expected);
        assert!(desc.is_leaf());
        assert!(!FieldDesc::new("value", desc).optional);
    }
    assert!(!Desc::default().nullable);
}

#[test]
fn test_simple_struct_rendering() {
    let desc = Desc::structure(vec![
        FieldDesc::new("name", Desc::plain("string")).with_doc("The name"),
        FieldDesc::new("count", Desc::plain("u32")),
        FieldDesc::new("enabled", Desc::plain("bool")).optional().with_doc("Whether enabled"),
    ]);

    let output = desc.display();
    assert!(output.contains("◆ name: string"));
    assert!(output.contains("◆ count: u32"));
    assert!(output.contains("◇ enabled: bool"));
    assert!(output.contains("‣ The name"));
    assert!(output.contains("‣ Whether enabled"));
}

#[test]
fn without_default_variant_only_changes_a_direct_enum() {
    let direct = Desc::enumeration(vec![
        VariantDesc::unit("summary", true),
        VariantDesc::unit("full", false),
    ])
    .without_default_variant();
    assert!(direct.unit_enum_variants().unwrap().iter().all(|variant| !variant.is_default()));

    let nested = Desc::list(Desc::enumeration(vec![
        VariantDesc::unit("summary", true),
        VariantDesc::unit("full", false),
    ]))
    .without_default_variant();
    let DescKind::List { item } = nested.kind else {
        panic!("expected a list description");
    };
    assert!(item.unit_enum_variants().unwrap()[0].is_default());
}

#[test]
fn test_nested_struct_rendering() {
    let inner = Desc::structure(vec![
        FieldDesc::new("host", Desc::plain("string")),
        FieldDesc::new("port", Desc::plain("u16")).with_default("8080"),
    ]);

    let desc = Desc::structure(vec![
        FieldDesc::new("server", inner),
        FieldDesc::new("debug", Desc::plain("bool")),
    ]);

    let output = desc.display();
    assert!(output.contains("◆ server"));
    assert!(output.contains("◆ host: string"));
    assert!(output.contains("◇ port: u16 → 8080"));
    assert!(output.contains("◆ debug: bool"));
}

#[test]
fn test_enum_rendering() {
    let desc = Desc::enumeration(vec![
        VariantDesc::unit("auto", true).with_doc("Automatic mode"),
        VariantDesc::unit("manual", false),
        VariantDesc::payload(
            "custom",
            false,
            Desc::structure(vec![FieldDesc::new("value", Desc::plain("u32"))]),
        ),
    ]);

    let output = desc.display();
    assert!(output.contains("» auto"));
    assert!(output.contains("› manual"));
    assert!(output.contains("› custom"));
    assert!(output.contains("‣ Automatic mode"));
}

#[test]
fn test_list_rendering() {
    let desc = Desc::structure(vec![
        FieldDesc::new("tags", Desc::list(Desc::plain("string"))),
        FieldDesc::new(
            "items",
            Desc::list(Desc::structure(vec![FieldDesc::new("id", Desc::plain("u32"))])),
        ),
    ]);

    let output = desc.display();
    assert!(output.contains("tags: list[string]"));
    assert!(output.contains("items: list"));
    assert!(output.contains("◆ id: u32"));
}

#[test]
fn test_type_label() {
    assert_eq!(Desc::plain("u32").type_label(), "u32");
    assert_eq!(Desc::default().type_label(), "");
    assert_eq!(Desc::list(Desc::plain("string")).type_label(), "list[string]");
    assert_eq!(Desc::list(Desc::default()).type_label(), "list");
}

// Path traversal tests

#[test]
fn test_entry_at_path_struct() {
    let desc = Desc::structure(vec![
        FieldDesc::new("name", Desc::plain("string")),
        FieldDesc::new(
            "server",
            Desc::structure(vec![
                FieldDesc::new("host", Desc::plain("string")),
                FieldDesc::new("port", Desc::plain("u16")),
            ]),
        ),
    ]);

    // Direct field
    let entry = desc.entry_at_path("name").unwrap();
    assert!(matches!(entry, EntryRef::Field(f) if f.name == "name"));

    // Nested field
    let entry = desc.entry_at_path("server.host").unwrap();
    assert!(matches!(entry, EntryRef::Field(f) if f.name == "host"));

    // Missing field
    assert!(desc.entry_at_path("missing").is_none());
    assert!(desc.entry_at_path("server.missing").is_none());
}

#[test]
fn test_entry_at_path_enum() {
    let desc = Desc::enumeration(vec![
        VariantDesc::unit("auto", true),
        VariantDesc::payload(
            "custom",
            false,
            Desc::structure(vec![FieldDesc::new("value", Desc::plain("u32"))]),
        ),
    ]);

    // Unit variant
    let entry = desc.entry_at_path("auto").unwrap();
    assert!(matches!(entry, EntryRef::Variant(v) if v.name == "auto"));

    // Payload variant's field
    let entry = desc.entry_at_path("custom.value").unwrap();
    assert!(matches!(entry, EntryRef::Field(f) if f.name == "value"));

    // Can't traverse into unit variant
    assert!(desc.entry_at_path("auto.something").is_none());
}

#[test]
fn test_validate_path_success() {
    let desc = Desc::structure(vec![
        FieldDesc::new("name", Desc::plain("string")),
        FieldDesc::new(
            "server",
            Desc::structure(vec![FieldDesc::new("host", Desc::plain("string"))]),
        ),
    ]);

    assert!(desc.validate_path("name").is_ok());
    assert!(desc.validate_path("server").is_ok());
    assert!(desc.validate_path("server.host").is_ok());
}

#[test]
fn test_validate_path_failure() {
    let desc = Desc::structure(vec![FieldDesc::new("name", Desc::plain("string"))]);

    let err = desc.validate_path("missing").unwrap_err();
    match err {
        DescPathError::UnknownPath(UnknownPathError {
            invalid_key,
            valid_prefix,
            ..
        }) => {
            assert_eq!(invalid_key, "missing");
            assert!(valid_prefix.is_empty());
        }
        DescPathError::InvalidSyntax(_) => panic!("expected UnknownPath"),
    }
}

#[test]
fn test_validate_path_nested_failure() {
    let desc = Desc::structure(vec![FieldDesc::new(
        "server",
        Desc::structure(vec![FieldDesc::new("host", Desc::plain("string"))]),
    )]);

    let err = desc.validate_path("server.missing").unwrap_err();
    match err {
        DescPathError::UnknownPath(UnknownPathError {
            invalid_key,
            valid_prefix,
            ..
        }) => {
            assert_eq!(invalid_key, "missing");
            assert_eq!(valid_prefix, "server");
        }
        DescPathError::InvalidSyntax(_) => panic!("expected UnknownPath"),
    }
}

#[test]
fn test_validate_path_unit_variant_blocks_deeper() {
    let desc = Desc::enumeration(vec![VariantDesc::unit("auto", true)]);

    // Selecting unit variant is fine
    assert!(desc.validate_path("auto").is_ok());

    // But can't go deeper
    let err = desc.validate_path("auto.something").unwrap_err();
    match err {
        DescPathError::UnknownPath(UnknownPathError { valid_prefix, .. }) => {
            assert_eq!(valid_prefix, "auto");
        }
        DescPathError::InvalidSyntax(_) => panic!("expected UnknownPath"),
    }
}

#[test]
fn test_display_with_max_depth() {
    // Create a nested structure: root { level1 { level2 { level3: scalar } } }
    let desc = Desc::structure(vec![FieldDesc::new(
        "level1",
        Desc::structure(vec![FieldDesc::new(
            "level2",
            Desc::structure(vec![FieldDesc::new("level3", Desc::plain("string"))]),
        )]),
    )]);

    // With max_depth=1, should only show level1, not level2 or level3
    let config = DisplayConfig::default().with_max_depth(1);
    let output = desc.display_with(&config);

    assert!(output.contains("level1"), "Should contain level1");
    assert!(!output.contains("level2"), "Should not contain level2 (beyond depth 1)");
    assert!(!output.contains("level3"), "Should not contain level3 (beyond depth 1)");
}

#[test]
fn test_display_with_max_depth_2() {
    // Create a nested structure: root { level1 { level2 { level3: scalar } } }
    let desc = Desc::structure(vec![FieldDesc::new(
        "level1",
        Desc::structure(vec![FieldDesc::new(
            "level2",
            Desc::structure(vec![FieldDesc::new("level3", Desc::plain("string"))]),
        )]),
    )]);

    // With max_depth=2, should show level1 and level2, but not level3
    let config = DisplayConfig::default().with_max_depth(2);
    let output = desc.display_with(&config);

    assert!(output.contains("level1"), "Should contain level1");
    assert!(output.contains("level2"), "Should contain level2");
    assert!(!output.contains("level3"), "Should not contain level3 (beyond depth 2)");
}

#[test]
fn test_display_unlimited_depth() {
    // Create a nested structure: root { level1 { level2 { level3: scalar } } }
    let desc = Desc::structure(vec![FieldDesc::new(
        "level1",
        Desc::structure(vec![FieldDesc::new(
            "level2",
            Desc::structure(vec![FieldDesc::new("level3", Desc::plain("string"))]),
        )]),
    )]);

    // With no max_depth, should show all levels
    let output = desc.display();

    assert!(output.contains("level1"), "Should contain level1");
    assert!(output.contains("level2"), "Should contain level2");
    assert!(output.contains("level3"), "Should contain level3");
}

#[test]
fn test_path_error_shows_only_immediate_children() {
    // Create a nested structure where the error will occur at root level
    let desc = Desc::structure(vec![
        FieldDesc::new(
            "alpha",
            Desc::structure(vec![FieldDesc::new("nested", Desc::plain("string"))]),
        ),
        FieldDesc::new("beta", Desc::plain("string")),
    ]);

    let err = desc.validate_path("invalid").unwrap_err();
    let error_display = err.to_string();

    // Should show the immediate children (alpha, beta) but not nested children
    assert!(error_display.contains("alpha"), "Should show immediate child 'alpha'");
    assert!(error_display.contains("beta"), "Should show immediate child 'beta'");
    assert!(!error_display.contains("nested"), "Should not show nested children (depth limited)");
}
