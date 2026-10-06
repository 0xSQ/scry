use super::*;

// ---------------------------------------------------------------------------------------------- //

#[test]
fn explicit_expression_is_a_field_fallback() {
    let field: syn::Field = syn::parse_quote! {
        #[scry(default = Vec::new())]
        values: Vec<String>
    };

    let attrs = FieldAttrs::from_attrs(&field.attrs).unwrap();

    assert!(matches!(attrs.fallback, FieldFallback::Expression(_)));
}

#[test]
fn from_defaults_is_a_field_fallback() {
    let field: syn::Field = syn::parse_quote! {
        #[scry(from_defaults)]
        child: Child
    };

    let attrs = FieldAttrs::from_attrs(&field.attrs).unwrap();

    assert!(matches!(attrs.fallback, FieldFallback::FromDefaults));
}

#[test]
fn bare_default_explains_both_replacements() {
    let field: syn::Field = syn::parse_quote! {
        #[scry(default)]
        child: Child
    };

    let error = field_attrs_error(&field);

    assert!(error.contains("default = EXPR"));
    assert!(error.contains("from_defaults"));
}

#[test]
fn conflicting_fallbacks_are_rejected() {
    let field: syn::Field = syn::parse_quote! {
        #[scry(default = Child::new(), from_defaults)]
        child: Child
    };

    let error = field_attrs_error(&field);

    assert!(error.contains("conflicting Scry field fallbacks"));
}

#[test]
fn duplicate_fallbacks_are_rejected() {
    let field: syn::Field = syn::parse_quote! {
        #[scry(from_defaults, from_defaults)]
        child: Child
    };

    let error = field_attrs_error(&field);

    assert!(error.contains("duplicate `from_defaults` field fallback"));
}

#[test]
fn from_defaults_on_option_is_rejected() {
    let input: DeriveInput = syn::parse_quote! {
        struct Parent {
            #[scry(from_defaults)]
            child: Option<Child>,
        }
    };

    let error = match parse_input(&input) {
        Ok(_) => panic!("expected `from_defaults` on `Option<T>` to be rejected"),
        Err(error) => error.to_string(),
    };

    assert!(error.contains("not supported on `Option<T>` fields"));
    assert!(error.contains("implicit `None` fallback"));
}

#[test]
fn duplicate_field_renames_are_rejected_within_and_across_attributes() {
    let fields: [syn::Field; 2] = [
        syn::parse_quote! {
            #[scry(rename = "first", rename = "second")]
            value: String
        },
        syn::parse_quote! {
            #[scry(rename = "first")]
            #[scry(rename = "second")]
            value: String
        },
    ];

    for field in fields {
        assert!(field_attrs_error(&field).contains("duplicate `rename` field attribute"));
    }
}

#[test]
fn duplicate_variant_renames_are_rejected_within_and_across_attributes() {
    let inputs: [DeriveInput; 2] = [
        syn::parse_quote! {
            enum Mode {
                #[scry(rename = "first", rename = "second")]
                Value,
            }
        },
        syn::parse_quote! {
            enum Mode {
                #[scry(rename = "first")]
                #[scry(rename = "second")]
                Value,
            }
        },
    ];

    for input in inputs {
        assert!(parse_input_error(&input).contains("duplicate `rename` variant attribute"));
    }
}

#[test]
fn duplicate_effective_field_keys_are_rejected_in_each_named_scope() {
    let inputs: [DeriveInput; 3] = [
        syn::parse_quote! {
            struct Duplicate {
                #[scry(rename = "shared")]
                first: String,
                #[scry(rename = "shared")]
                second: String,
            }
        },
        syn::parse_quote! {
            struct Duplicate {
                shared: String,
                #[scry(rename = "shared")]
                second: String,
            }
        },
        syn::parse_quote! {
            enum Duplicate {
                Value {
                    shared: String,
                    #[scry(rename = "shared")]
                    second: String,
                },
            }
        },
    ];

    for input in inputs {
        assert!(parse_input_error(&input).contains("duplicate Scry field key \"shared\""));
        let error = parse_input(&input).err().expect("duplicate fields should fail");
        assert_eq!(error.into_iter().count(), 2, "both declarations should be identified");
    }
}

#[test]
fn field_keys_are_literal_case_sensitive_and_scoped_to_each_payload() {
    let input: DeriveInput = syn::parse_quote! {
        enum Distinct {
            First {
                #[scry(rename = "model.version")]
                dotted: String,
                #[scry(rename = "model")]
                plain: String,
                #[scry(rename = "items[0]")]
                bracketed: String,
                #[scry(rename = "odd key")]
                spaced: String,
                #[scry(rename = "")]
                empty: String,
                #[scry(rename = "UPPER")]
                upper: String,
                #[scry(rename = "upper")]
                lower: String,
            },
            Second {
                #[scry(rename = "model.version")]
                dotted: String,
            },
        }
    };

    assert!(parse_input(&input).is_ok());
}

#[test]
fn canonical_variant_names_are_unique_across_unit_and_payload_shapes() {
    let inputs: [DeriveInput; 3] = [
        syn::parse_quote! {
            enum Duplicate {
                #[scry(rename = "shared")]
                First,
                #[scry(rename = "shared")]
                Second,
            }
        },
        syn::parse_quote! {
            enum Duplicate {
                #[scry(rename = "shared")]
                First(String),
                #[scry(rename = "shared")]
                Second { value: String },
            }
        },
        syn::parse_quote! {
            enum Duplicate {
                #[scry(rename = "shared")]
                First,
                #[scry(rename = "shared")]
                Second(String),
            }
        },
    ];

    for input in inputs {
        assert!(parse_input_error(&input).contains("duplicate Scry variant name \"shared\""));
    }
}

#[test]
fn unit_variant_case_and_spelling_alias_collisions_are_rejected() {
    let inputs: [DeriveInput; 3] = [
        syn::parse_quote! {
            enum Duplicate {
                #[scry(rename = "FAST")]
                First,
                #[scry(rename = "fast")]
                Second,
            }
        },
        syn::parse_quote! {
            enum Duplicate {
                #[scry(rename = "foo-bar")]
                First,
                #[scry(rename = "foo_bar")]
                Second,
            }
        },
        syn::parse_quote! {
            #[scry(rename_all = "kebab-case")]
            enum Duplicate {
                FooBar,
                #[scry(rename = "foo_bar")]
                Other,
            }
        },
    ];

    for input in inputs {
        assert!(parse_input_error(&input).contains("conflicting Scry unit variant spelling"));
    }
}

#[test]
fn payload_variant_spelling_alias_collisions_are_rejected() {
    let input: DeriveInput = syn::parse_quote! {
        enum Duplicate {
            #[scry(rename = "foo-bar")]
            First(String),
            #[scry(rename = "foo_bar")]
            Second { value: String },
        }
    };

    assert!(parse_input_error(&input).contains("conflicting Scry payload variant spelling"));
}

#[test]
fn payload_case_and_cross_shape_aliases_remain_distinct() {
    let input: DeriveInput = syn::parse_quote! {
        enum Distinct {
            #[scry(rename = "FAST")]
            Upper(String),
            #[scry(rename = "fast")]
            Lower(String),
            #[scry(rename = "foo-bar")]
            Unit,
            #[scry(rename = "foo_bar")]
            Payload(String),
        }
    };

    assert!(parse_input(&input).is_ok());
}

#[test]
fn enum_accepts_one_scry_default_unit_variant() {
    let input: DeriveInput = syn::parse_quote! {
        enum OutputMode {
            #[scry(default)]
            Summary,
            Full,
        }
    };

    let DeriveTarget::Enum(info) = parse_input(&input).unwrap() else {
        panic!("expected an enum");
    };

    assert!(info.variants[0].attrs.is_default);
    assert!(!info.variants[1].attrs.is_default);
}

#[test]
fn enum_rejects_multiple_scry_default_variants() {
    let input: DeriveInput = syn::parse_quote! {
        enum OutputMode {
            #[scry(default)]
            Summary,
            #[scry(default)]
            Full,
        }
    };

    let error = parse_input_error(&input);

    assert!(error.contains("multiple `#[scry(default)]` enum variants"));
    assert!(error.contains("exactly one unit variant"));
}

#[test]
fn enum_rejects_a_duplicate_marker_on_one_variant() {
    let input: DeriveInput = syn::parse_quote! {
        enum OutputMode {
            #[scry(default, default)]
            Summary,
        }
    };

    let error = parse_input_error(&input);

    assert!(error.contains("duplicate `#[scry(default)]` marker"));
}

#[test]
fn enum_rejects_a_default_payload_variant() {
    let input: DeriveInput = syn::parse_quote! {
        enum OutputMode {
            #[scry(default)]
            Custom(String),
        }
    };

    let error = parse_input_error(&input);

    assert!(error.contains("only supported on unit enum variants"));
}

#[test]
fn rust_default_marker_is_unrelated() {
    let input: DeriveInput = syn::parse_quote! {
        enum OutputMode {
            #[default]
            Summary,
            Full,
        }
    };

    let DeriveTarget::Enum(info) = parse_input(&input).unwrap() else {
        panic!("expected an enum");
    };

    assert!(info.variants.iter().all(|variant| !variant.attrs.is_default));
}

fn field_attrs_error(field: &syn::Field) -> String {
    match FieldAttrs::from_attrs(&field.attrs) {
        Ok(_) => panic!("expected field attributes to be rejected"),
        Err(error) => error.to_string(),
    }
}

fn parse_input_error(input: &DeriveInput) -> String {
    match parse_input(input) {
        Ok(_) => panic!("expected derive input to be rejected"),
        Err(error) => error.to_string(),
    }
}
