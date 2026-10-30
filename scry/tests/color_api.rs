//! Checks color notation, byte-preserving values, and opacity conversions.

use scry::kit::color::{named, NonOpaqueError, ParseError, Rgb8, Rgba8};

// ---------------------------------------------------------------------------------------------- //

#[test]
fn constructors_and_standard_conversions_preserve_channels() {
    const RGB: Rgb8 = Rgb8::new(32, 128, 224);
    const RGBA: Rgba8 = Rgba8::new(32, 128, 224, 64);
    assert_eq!((RGB.red(), RGB.green(), RGB.blue()), (32, 128, 224));
    assert_eq!((RGBA.red(), RGBA.green(), RGBA.blue(), RGBA.alpha()), (32, 128, 224, 64));
    assert_eq!(RGB.as_array(), &[32, 128, 224]);
    assert_eq!(RGBA.as_array(), &[32, 128, 224, 64]);
    assert_eq!(RGB.into_array(), [32, 128, 224]);
    assert_eq!(RGBA.into_array(), [32, 128, 224, 64]);

    assert_eq!(Rgb8::from([32, 128, 224]), RGB);
    assert_eq!(Rgb8::from((32, 128, 224)), RGB);
    assert_eq!(<[u8; 3]>::from(RGB), [32, 128, 224]);
    assert_eq!(<(u8, u8, u8)>::from(RGB), (32, 128, 224));
    assert_eq!(Rgba8::from([32, 128, 224, 64]), RGBA);
    assert_eq!(Rgba8::from((32, 128, 224, 64)), RGBA);
    assert_eq!(<[u8; 4]>::from(RGBA), [32, 128, 224, 64]);
    assert_eq!(<(u8, u8, u8, u8)>::from(RGBA), (32, 128, 224, 64));
    assert_eq!(Rgba8::from(RGB), Rgba8::new(32, 128, 224, 255));
}

#[test]
fn named_constants_are_opaque_const_values_with_css_aliases() {
    const BACKGROUND: Rgb8 = named::TEAL;
    assert_eq!(BACKGROUND.into_array(), [0, 128, 128]);
    assert_eq!(named::REBECCAPURPLE.into_array(), [102, 51, 153]);
    assert_eq!(named::LIGHTGOLDENRODYELLOW.into_array(), [250, 250, 210]);
    assert_eq!(named::AQUA, named::CYAN);
    assert_eq!(named::FUCHSIA, named::MAGENTA);
    assert_eq!(named::GRAY, named::GREY);
    assert_eq!(Rgba8::from(BACKGROUND).into_array(), [0, 128, 128, 255]);
    assert_eq!("TeAl".parse::<Rgb8>().unwrap(), BACKGROUND);
}

#[test]
fn css_names_are_case_insensitive_and_have_defined_srgb_values() {
    for (name, expected) in [
        ("red", [255, 0, 0]),
        ("TeAl", [0, 128, 128]),
        ("green", [0, 128, 0]),
        ("lime", [0, 255, 0]),
        ("rebeccapurple", [102, 51, 153]),
        ("papayawhip", [255, 239, 213]),
        ("lightgoldenrodyellow", [250, 250, 210]),
        ("gray", [128, 128, 128]),
        ("grey", [128, 128, 128]),
        ("darkslategray", [47, 79, 79]),
        ("darkslategrey", [47, 79, 79]),
        ("cyan", [0, 255, 255]),
        ("aqua", [0, 255, 255]),
        ("magenta", [255, 0, 255]),
        ("fuchsia", [255, 0, 255]),
    ] {
        let rgb: Rgb8 = name.parse().unwrap();
        assert_eq!(rgb.into_array(), expected, "{name}");
        assert_eq!(name.parse::<Rgba8>().unwrap(), Rgba8::from(rgb), "{name}");
    }
    assert_eq!("\t RED\r\n".parse::<Rgb8>().unwrap(), Rgb8::new(255, 0, 0));
    assert_eq!(" TrAnSpArEnT ".parse::<Rgba8>().unwrap(), Rgba8::new(0, 0, 0, 0));
}

#[test]
fn hex_forms_expand_channels_and_keep_alpha_last() {
    for (text, expected) in [
        ("#28e", [34, 136, 238, 255]),
        ("#28e8", [34, 136, 238, 136]),
        ("#2080E0", [32, 128, 224, 255]),
        ("#2080e040", [32, 128, 224, 64]),
        ("#1234", [17, 34, 51, 68]),
        ("#01020304", [1, 2, 3, 4]),
        ("#f00f", [255, 0, 0, 255]),
        ("#ff000000", [255, 0, 0, 0]),
        ("\t #00f \n", [0, 0, 255, 255]),
    ] {
        assert_eq!(text.parse::<Rgba8>().unwrap().into_array(), expected, "{text}");
    }
    assert_eq!("#f00f".parse::<Rgb8>().unwrap(), Rgb8::new(255, 0, 0));
    assert_eq!("#ff0000ff".parse::<Rgb8>().unwrap(), Rgb8::new(255, 0, 0));
}

#[test]
fn string_parsing_rejects_unadvertised_forms_and_non_ascii_content() {
    for text in [
        "",
        " ",
        "#",
        "#12",
        "#12345",
        "#1234567",
        "#123456789",
        "#ggg",
        "#12x4",
        "#11223z",
        "#1122334z",
        "ff0000",
        "0xff0000",
        "rgb(255, 0, 0)",
        "hsl(0 100% 50%)",
        "[255, 0, 0]",
        "currentColor",
        "auto",
        "dark slate gray",
        "#éa",
        "#éab",
        "#💡ab",
        "réD",
        "\u{2003}red\u{2003}",
    ] {
        let _: ParseError = text.parse::<Rgba8>().expect_err(text);
        let _: ParseError = text.parse::<Rgb8>().expect_err(text);
    }
}

#[test]
fn opaque_conversion_checks_every_alpha_value_without_losing_rgb() {
    for alpha in u8::MIN..=u8::MAX {
        let rgba = Rgba8::new(32, 128, 224, alpha);
        if alpha == 255 {
            assert_eq!(Rgb8::try_from(rgba).unwrap(), Rgb8::new(32, 128, 224));
            assert_eq!(rgba.to_string().parse::<Rgb8>().unwrap(), Rgb8::new(32, 128, 224));
        } else {
            let error: NonOpaqueError = Rgb8::try_from(rgba).unwrap_err();
            assert_eq!(error.alpha(), alpha);
            assert!(rgba.to_string().parse::<Rgb8>().is_err());
        }
    }
    assert!("transparent".parse::<Rgb8>().is_err());
}

#[test]
fn display_normalizes_notation_and_round_trips_exact_byte_values() {
    assert_eq!("RED".parse::<Rgb8>().unwrap().to_string(), "#ff0000");
    assert_eq!("#AbC".parse::<Rgba8>().unwrap().to_string(), "#aabbcc");
    assert_eq!("#AbC8".parse::<Rgba8>().unwrap().to_string(), "#aabbcc88");
    assert_eq!(Rgba8::new(255, 0, 0, 0).to_string(), "#ff000000");
    assert_ne!(Rgba8::new(255, 0, 0, 0), Rgba8::new(0, 0, 0, 0));

    for red in [0, 1, 17, 127, 128, 254, 255] {
        for green in [0, 1, 17, 127, 128, 254, 255] {
            for blue in [0, 1, 17, 127, 128, 254, 255] {
                let rgb = Rgb8::new(red, green, blue);
                assert_eq!(rgb.to_string().parse::<Rgb8>().unwrap(), rgb);
                for alpha in [0, 1, 17, 127, 128, 254, 255] {
                    let rgba = Rgba8::new(red, green, blue, alpha);
                    assert_eq!(rgba.to_string().parse::<Rgba8>().unwrap(), rgba);
                }
            }
        }
    }
}
