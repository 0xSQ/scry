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
fn duplicate_field_hooks_are_rejected_in_named_and_positional_shapes() {
    for hook_name in ["from_node_with", "to_node_with", "describe_with"] {
        let hook = Ident::new(hook_name, Span::call_site());
        for second_path in [
            syn::parse_quote!(adapter::first),
            syn::parse_quote!(adapter::second),
        ] {
            let second_path: syn::Path = second_path;
            for separate_attributes in [false, true] {
                let attrs: Vec<Attribute> = if separate_attributes {
                    vec![
                        syn::parse_quote!(#[scry(#hook(adapter::first))]),
                        syn::parse_quote!(#[scry(#hook(#second_path))]),
                    ]
                } else {
                    vec![syn::parse_quote!(#[scry(#hook(adapter::first), #hook(#second_path))])]
                };
                let inputs: [DeriveInput; 4] = [
                    syn::parse_quote!(struct Value { #(#attrs)* value: Foreign }),
                    syn::parse_quote!(struct Value(#(#attrs)* Foreign);),
                    syn::parse_quote!(enum Value { Payload { #(#attrs)* value: Foreign } }),
                    syn::parse_quote!(enum Value { Payload(#(#attrs)* Foreign) }),
                ];

                for input in inputs {
                    let error =
                        parse_input(&input).err().expect("duplicate field hooks should fail");
                    assert_conflicting_attribute_error(
                        error,
                        &format!("duplicate `{hook_name}` field attribute"),
                    );
                }
            }
        }
    }
}

#[test]
fn duplicate_enum_rename_all_is_rejected_within_and_across_attributes() {
    for second_case in ["snake_case", "kebab-case"] {
        for separate_attributes in [false, true] {
            let attrs: Vec<Attribute> = if separate_attributes {
                vec![
                    syn::parse_quote!(#[scry(rename_all = "snake_case")]),
                    syn::parse_quote!(#[scry(rename_all = #second_case)]),
                ]
            } else {
                vec![
                    syn::parse_quote!(#[scry(rename_all = "snake_case", rename_all = #second_case)]),
                ]
            };
            let input: DeriveInput = syn::parse_quote! {
                #(#attrs)*
                enum Mode { FirstValue, SecondValue }
            };

            let error = parse_input(&input).err().expect("duplicate enum rename_all should fail");
            assert_conflicting_attribute_error(error, "duplicate `rename_all` enum attribute");
        }
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

#[test]
fn generic_declarations_defaults_and_where_clauses_are_preserved() {
    let inputs: [DeriveInput; 2] = [
        syn::parse_quote! {
            struct Container<'a, T: Clone = String, const N: usize = 2>
            where
                T: 'a,
            {
                values: &'a [T; N],
            }
        },
        syn::parse_quote! {
            enum Container<'a, T: Clone = String, const N: usize = 2>
            where
                T: 'a,
            {
                Values(&'a [T; N]),
            }
        },
    ];

    for input in inputs {
        let parsed = parse_input(&input).unwrap();
        let generics = match &parsed {
            DeriveTarget::Struct(info) => &info.generics,
            DeriveTarget::Enum(info) => &info.generics,
        };
        let original = &input.generics;
        let where_clause = &generics.where_clause;
        let original_where_clause = &original.where_clause;

        assert_eq!(quote::quote!(#generics).to_string(), quote::quote!(#original).to_string());
        assert_eq!(
            quote::quote!(#where_clause).to_string(),
            quote::quote!(#original_where_clause).to_string(),
        );
        assert!(generics.type_params().next().unwrap().default.is_some());
        assert!(generics.const_params().next().unwrap().default.is_some());
    }
}

#[test]
fn container_variant_and_field_doc_summaries_are_preserved_for_all_shapes() {
    let named: DeriveInput = syn::parse_quote! {
        /// Named container.
        ///
        /// Additional explanation.
        struct Named {
            /// Named value.
            /// More summary text.
            value: u16,
        }
    };
    let DeriveTarget::Struct(info) = parse_input(&named).unwrap() else {
        panic!("expected a struct");
    };
    let StructFields::Named(fields) = info.fields else {
        panic!("expected named fields");
    };
    assert_eq!(info.doc, "Named container.");
    assert_eq!(fields[0].doc, "Named value. More summary text.");

    let tuple: DeriveInput = syn::parse_quote! {
        /// Positional container.
        struct Tuple(
            /// First value.
            u16,
            /// Second value.
            String,
        );
    };
    let DeriveTarget::Struct(info) = parse_input(&tuple).unwrap() else {
        panic!("expected a struct");
    };
    let StructFields::Tuple(fields) = info.fields else {
        panic!("expected positional fields");
    };
    assert_eq!(info.doc, "Positional container.");
    assert_eq!(fields[0].doc, "First value.");
    assert_eq!(fields[1].doc, "Second value.");
    assert!(matches!(fields[0].member, Member::Unnamed(syn::Index { index: 0, .. })));
    assert!(matches!(fields[1].member, Member::Unnamed(syn::Index { index: 1, .. })));

    let variants: DeriveInput = syn::parse_quote! {
        /// Enum container.
        enum Container {
            /// Unit variant.
            Empty,
            /// Positional variant.
            Values(
                /// Variant value.
                u16,
            ),
            /// Named variant.
            Named {
                /// Variant field.
                value: u16,
            },
        }
    };
    let DeriveTarget::Enum(info) = parse_input(&variants).unwrap() else {
        panic!("expected an enum");
    };
    assert_eq!(info.doc, "Enum container.");
    assert_eq!(info.variants[0].doc, "Unit variant.");
    assert_eq!(info.variants[1].doc, "Positional variant.");
    assert_eq!(info.variants[2].doc, "Named variant.");
    let VariantData::Tuple(fields) = &info.variants[1].data else {
        panic!("expected positional variant fields");
    };
    assert_eq!(fields[0].doc, "Variant value.");
    let VariantData::Struct(fields) = &info.variants[2].data else {
        panic!("expected named variant fields");
    };
    assert_eq!(fields[0].doc, "Variant field.");
}

#[test]
fn fields_accept_one_hook_per_operation() {
    let inputs: [DeriveInput; 4] = [
        syn::parse_quote! {
            struct Value {
                #[scry(from_node_with(adapter::read))]
                #[scry(to_node_with(adapter::write))]
                #[scry(describe_with(adapter::describe))]
                value: Foreign,
            }
        },
        syn::parse_quote! {
            struct Value(
                #[scry(from_node_with(adapter::read), to_node_with(adapter::write),
                       describe_with(adapter::describe))]
                Foreign,
            );
        },
        syn::parse_quote! {
            enum Value {
                Payload {
                    #[scry(from_node_with(adapter::read), to_node_with(adapter::write),
                           describe_with(adapter::describe))]
                    value: Foreign,
                },
            }
        },
        syn::parse_quote! {
            enum Value {
                Payload(
                    u16,
                    #[scry(from_node_with(adapter::read), to_node_with(adapter::write),
                           describe_with(adapter::describe))]
                    Foreign,
                ),
            }
        },
    ];

    for input in inputs {
        let parsed = parse_input(&input).unwrap();
        let fields = match &parsed {
            DeriveTarget::Struct(StructInfo {
                fields: StructFields::Named(fields) | StructFields::Tuple(fields),
                ..
            }) => fields,
            DeriveTarget::Enum(info) => match &info.variants[0].data {
                VariantData::Struct(fields) | VariantData::Tuple(fields) => fields,
                _ => panic!("expected variant fields"),
            },
        };
        let attrs = &fields.last().unwrap().attrs;
        let from = &attrs.from_node_with;
        let to = &attrs.to_node_with;
        let describe = &attrs.describe_with;
        assert_eq!(quote::quote!(#from).to_string(), "adapter :: read");
        assert_eq!(quote::quote!(#to).to_string(), "adapter :: write");
        assert_eq!(quote::quote!(#describe).to_string(), "adapter :: describe");
    }
}

#[test]
fn positional_rename_and_fallbacks_have_targeted_errors() {
    let inputs: [(DeriveInput, &str); 6] = [
        (syn::parse_quote! { struct Value(#[scry(rename = "key")] u16); }, "rename"),
        (syn::parse_quote! { enum Value { Payload(#[scry(rename = "key")] u16) } }, "rename"),
        (syn::parse_quote! { struct Value(#[scry(default = 0)] u16); }, "default = EXPR"),
        (syn::parse_quote! { enum Value { Payload(#[scry(default = 0)] u16) } }, "default = EXPR"),
        (syn::parse_quote! { struct Value(#[scry(from_defaults)] Option<u16>); }, "from_defaults"),
        (
            syn::parse_quote! { enum Value { Payload(#[scry(from_defaults)] Option<u16>) } },
            "from_defaults",
        ),
    ];

    for (input, attribute) in inputs {
        let error = parse_input_error(&input);
        assert!(error.contains(&format!("`{attribute}` is not supported on positional fields")));
        if attribute == "rename" {
            assert!(error.contains("positional fields use indices"));
        } else {
            assert!(error.contains("positional fields must be present"));
        }
    }
}

#[test]
fn named_member_and_literal_configuration_key_are_distinct() {
    let input: DeriveInput = syn::parse_quote! {
        struct Value {
            #[scry(rename = "server.port")]
            port: u16,
            name: String,
        }
    };
    let DeriveTarget::Struct(info) = parse_input(&input).unwrap() else {
        panic!("expected a struct");
    };
    let StructFields::Named(fields) = info.fields else {
        panic!("expected named fields");
    };

    assert!(matches!(&fields[0].member, Member::Named(ident) if ident == "port"));
    assert_eq!(fields[0].config_key(), "server.port");
    assert_eq!(fields[1].config_key(), "name");
}

#[test]
fn option_detection_requires_one_written_type_argument() {
    let options: [Type; 2] = [
        syn::parse_quote! { Option<u16> },
        syn::parse_quote! { std::option::Option<u16> },
    ];
    let other_types: [Type; 5] = [
        syn::parse_quote! { Option },
        syn::parse_quote! { Option<> },
        syn::parse_quote! { Option<u16, u32> },
        syn::parse_quote! { Option<'a> },
        syn::parse_quote! { Option<2> },
    ];

    assert!(options.iter().all(is_option_type));
    assert!(other_types.iter().all(|ty| !is_option_type(ty)));
}

#[test]
fn generic_parameter_named_option_can_use_scry_defaults() {
    let input: DeriveInput = syn::parse_quote! {
        struct Container<Option> {
            #[scry(from_defaults)]
            value: Option,
        }
    };

    assert!(parse_input(&input).is_ok());
}

fn assert_conflicting_attribute_error(error: syn::Error, message: &str) {
    let declarations: Vec<_> = error.into_iter().collect();

    assert_eq!(declarations.len(), 2, "both declarations should be identified");
    assert_eq!(declarations[0].to_string(), message);
    assert_eq!(declarations[1].to_string(), "first declaration is here");
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
