use indoc::indoc;

use super::*;

// ---------------------------------------------------------------------------------------------- //

#[test]
fn nullable_metadata_preserves_shapes_without_changing_help_labels_or_omission() {
    let cases = [
        (Desc::plain("u32"), "u32"),
        (Desc::default(), ""),
        (Desc::structure(vec![]), ""),
        (Desc::enumeration(vec![]), ""),
        (Desc::tuple(vec![]), "tuple"),
        (Desc::list(Desc::plain("u32")), "list[u32]"),
    ];
    for (desc, expected) in cases {
        assert!(!desc.nullable);
        let desc = desc.nullable().nullable();
        assert!(desc.nullable);
        assert_eq!(desc.type_label(), expected);
        assert!(desc.is_leaf());
        assert!(!FieldDesc::new("value", desc).is_optional());
    }
    assert!(!Desc::default().nullable);
}

#[test]
fn help_rendering_keeps_useful_hints_children_and_defaults() {
    let desc = Desc::structure(vec![
        FieldDesc::new(
            "comfy",
            Desc::structure(vec![
                FieldDesc::new("url", Desc::plain("string").nullable()).optional(),
                FieldDesc::new("keep", Desc::plain("bool"))
                    .with_fallback(DefaultDesc::text("false")),
            ])
            .nullable(),
        )
        .optional(),
        FieldDesc::new(
            "resume",
            Desc::enumeration(vec![
                VariantDesc::unit("latest"),
                VariantDesc::payload("directory", Desc::plain("path").nullable()),
                VariantDesc::payload(
                    "custom",
                    Desc::structure(vec![FieldDesc::new("count", Desc::plain("u32"))]).nullable(),
                ),
            ])
            .nullable(),
        )
        .optional(),
        FieldDesc::new(
            "samples",
            Desc::list(Desc::list(Desc::plain("u32").nullable()).nullable()).nullable(),
        ),
        FieldDesc::new(
            "pair",
            Desc::tuple(vec![
                Desc::plain("u32").nullable(),
                Desc::structure(vec![FieldDesc::new("count", Desc::plain("u32"))]).nullable(),
            ])
            .nullable(),
        ),
    ]);

    let expected = indoc! {"
        ◇ comfy
        ┊  ◇ url: string
        ┊  ◇ keep: bool → false
        ◇ resume
        ┊  › latest
        ┊  › directory: path
        ┊  › custom
        ┊     ◆ count: u32
        ◆ samples: list[list[u32]]
        ◆ pair: tuple
           ◆ 0: u32
           ◆ 1
              ◆ count: u32
    "};
    assert_eq!(desc.display(), expected);
    for max_depth in [1, 2, 3] {
        let config = DisplayConfig::default().with_max_depth(max_depth);
        assert_eq!(desc.clone().nullable().display_with(&config), desc.display_with(&config));
    }
}

#[test]
fn standalone_plain_help_preserves_hints_and_docs_regardless_of_nullability() {
    let plain = Desc::plain("u32").with_doc("Number of attempts.");
    assert_eq!(plain.display(), "u32   ‣ Number of attempts.\n");
    assert_eq!(plain.clone().nullable().display(), plain.display());
    assert_eq!(Desc::default().display(), "");
    assert_eq!(Desc::default().nullable().display(), "");
    assert_eq!(Desc::default().with_doc("Custom value.").display(), "   ‣ Custom value.\n");
    assert_eq!(Desc::plain("null").nullable().display(), "null\n");
    assert_eq!(Desc::plain("string | null").nullable().display(), "string | null\n");
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
fn field_defaults_do_not_replace_the_values_intrinsic_default() {
    let mode = Desc::enumeration(vec![VariantDesc::unit("summary"), VariantDesc::unit("full")])
        .with_type_default(DefaultDesc::variant("summary"));
    assert_eq!(mode.display(), "» summary\n› full\n");

    let desc = Desc::structure(vec![
        FieldDesc::new("required", mode.clone()),
        FieldDesc::new("fallback", mode).with_fallback(DefaultDesc::variant("full")),
    ]);
    assert_eq!(
        desc.display(),
        indoc! {"
            ◆ required
            ┊  › summary
            ┊  › full
            ◇ fallback
               › summary
               » full
        "}
    );
    for name in ["required", "fallback"] {
        let Some(EntryRef::Field(field)) = desc.entry_at_path(name) else {
            panic!("expected field");
        };
        assert_eq!(field.value.type_default.as_ref().unwrap().variant_name(), Some("summary"));
    }
    assert_eq!(desc.entry_at_path("required.summary").unwrap().display(), "› summary\n");
    assert_eq!(desc.entry_at_path("fallback.full").unwrap().display(), "» full\n");

    let required_error = desc.validate_path("required.missing").unwrap_err().to_string();
    assert!(required_error.contains("› summary"));
    assert!(!required_error.contains('»'));
    let fallback_error = desc.validate_path("fallback.missing").unwrap_err().to_string();
    assert!(fallback_error.contains("» full"));
    assert!(!fallback_error.contains("» summary"));
}

#[test]
fn test_nested_struct_rendering() {
    let inner = Desc::structure(vec![
        FieldDesc::new("host", Desc::plain("string")),
        FieldDesc::new("port", Desc::plain("u16")).with_fallback(DefaultDesc::text("8080")),
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
        VariantDesc::unit("auto").with_doc("Automatic mode"),
        VariantDesc::unit("manual"),
        VariantDesc::payload(
            "custom",
            Desc::structure(vec![FieldDesc::new("value", Desc::plain("u32"))]),
        ),
    ])
    .with_type_default(DefaultDesc::variant("auto"));

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
fn type_label_styles_preserve_their_policies_through_nested_lists() {
    let choices = Desc::enumeration(vec![VariantDesc::unit("first"), VariantDesc::unit("second")]);
    let payload = Desc::enumeration(vec![VariantDesc::payload("value", Desc::plain("u32"))]);
    let cases = [
        (Desc::plain("u32"), "u32", Some("u32")),
        (Desc::default(), "", None),
        (Desc::plain(""), "", None),
        (Desc::plain(" \n\t "), " \n\t ", None),
        (Desc::structure(vec![]), "", None),
        (Desc::enumeration(vec![]), "", Some("string")),
        (choices.clone(), "", Some("string")),
        (payload.clone(), "", None),
        (Desc::tuple(vec![Desc::plain("u32")]), "tuple", None),
        (Desc::list(Desc::plain("string")), "list[string]", Some("list[string]")),
        (
            Desc::list(Desc::list(Desc::plain("u32").nullable()).nullable()),
            "list[list[u32]]",
            Some("list[list[u32]]"),
        ),
        (Desc::list(Desc::default()), "list", None),
        (Desc::list(Desc::plain("")), "list", None),
        (Desc::list(Desc::plain(" \t ")), "list[ \t ]", None),
        (Desc::list(Desc::list(Desc::default())), "list[list]", None),
        (Desc::list(Desc::structure(vec![])), "list", None),
        (Desc::list(payload), "list", None),
        (Desc::list(Desc::tuple(vec![])), "list[tuple]", None),
        (Desc::list(Desc::list(choices)), "list[list]", Some("list[list[string]]")),
        (
            Desc::list(Desc::plain(" duration\r\nexpression\u{2028} ")),
            "list[ duration\r\nexpression\u{2028} ]",
            Some("list[duration expression]"),
        ),
    ];
    for (desc, description, template) in cases {
        assert_eq!(desc.type_label(), description, "{desc:?}");
        assert_eq!(desc.type_label_with(TypeLabelStyle::Template).as_deref(), template, "{desc:?}");
    }
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
        VariantDesc::unit("auto"),
        VariantDesc::payload(
            "custom",
            Desc::structure(vec![FieldDesc::new("value", Desc::plain("u32"))]),
        ),
    ])
    .with_type_default(DefaultDesc::variant("auto"));

    // Unit variant
    let entry = desc.entry_at_path("auto").unwrap();
    assert!(matches!(entry, EntryRef::Variant { variant, .. } if variant.name == "auto"));

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
fn invalid_nested_paths_preserve_literal_keys_and_each_collection_index() {
    let desc = Desc::structure(vec![FieldDesc::new(
        "service.list",
        Desc::list(Desc::tuple(vec![Desc::structure(vec![FieldDesc::new(
            "known",
            Desc::plain("u32"),
        )])])),
    )]);
    let error = desc.validate_path(r#"["service.list"][3][0].missing"#).unwrap_err();
    let DescPathError::UnknownPath(error) = error else {
        panic!("expected a valid path with an unknown field");
    };
    assert_eq!(error.valid_prefix, r#"["service.list"][3][0]"#);
    assert_eq!(error.invalid_key, "missing");
    assert!(error.to_string().contains("known: u32"));
}

#[test]
fn test_validate_path_unit_variant_blocks_deeper() {
    let desc = Desc::enumeration(vec![VariantDesc::unit("auto")]);

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

    assert_eq!(output, "◆ level1 …\n");
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

    assert_eq!(output, "◆ level1\n   ◆ level2 …\n");
}

#[test]
fn depth_marks_structured_lists_and_leaves_empty_and_plain_shapes_intact() {
    let desc = Desc::structure(vec![
        FieldDesc::new(
            "items",
            Desc::list(Desc::structure(vec![FieldDesc::new("id", Desc::plain("u32"))])),
        ),
        FieldDesc::new("tags", Desc::list(Desc::plain("string"))),
        FieldDesc::new("empty_struct", Desc::structure(vec![])),
        FieldDesc::new("empty_enum", Desc::enumeration(vec![])),
        FieldDesc::new("empty_tuple", Desc::tuple(vec![])),
        FieldDesc::new("empty_items", Desc::list(Desc::structure(vec![]))),
        FieldDesc::new("enabled", Desc::plain("bool")),
    ]);
    let config = DisplayConfig::default().with_max_depth(1);
    assert_eq!(
        desc.display_with(&config),
        indoc! {"
            ◆ items: list …
            ◆ tags: list[string]
            ◆ empty_struct
            ◆ empty_enum
            ◆ empty_tuple: tuple
            ◆ empty_items: list
            ◆ enabled: bool
        "}
    );
    assert_eq!(desc.display_with(&DisplayConfig::default().with_max_depth(2)), desc.display());
    assert_eq!(Desc::plain("string").display_with(&config), "string\n");
}

#[test]
fn depth_marks_tuple_elements_and_enum_payloads_at_their_displayed_levels() {
    let desc = Desc::structure(vec![
        FieldDesc::new(
            "pair",
            Desc::tuple(vec![
                Desc::plain("u32"),
                Desc::structure(vec![FieldDesc::new("count", Desc::plain("u32"))]),
                Desc::structure(vec![]),
            ]),
        ),
        FieldDesc::new(
            "mode",
            Desc::enumeration(vec![
                VariantDesc::unit("auto"),
                VariantDesc::payload("file", Desc::plain("path")),
                VariantDesc::payload(
                    "custom",
                    Desc::structure(vec![FieldDesc::new("count", Desc::plain("u32"))]),
                ),
                VariantDesc::payload("empty", Desc::structure(vec![])),
            ]),
        )
        .with_fallback(DefaultDesc::variant("auto")),
    ]);
    assert_eq!(
        desc.display_with(&DisplayConfig::default().with_max_depth(1)),
        "◆ pair: tuple …\n◇ mode …\n"
    );
    assert_eq!(
        desc.display_with(&DisplayConfig::default().with_max_depth(2)),
        indoc! {"
            ◆ pair: tuple
            ┊  ◆ 0: u32
            ┊  ◆ 1 …
            ┊  ◆ 2
            ◇ mode
               » auto
               › file: path
               › custom …
               › empty
        "}
    );
    assert_eq!(desc.display_with(&DisplayConfig::default().with_max_depth(3)), desc.display());
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
