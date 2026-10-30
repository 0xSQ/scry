# Colors

A color setting should be easy to recognize and comfortable to write. Say you're building a
viewer with a canvas background and a translucent selection overlay. An author might choose
explicit channels for the background and a hex code for the overlay:

```rhai
#{
    background: [24, 24, 28],
    selection: "#2080e080",
}
```

The same background can be written as `"#18181c"`. A familiar named color such as `"teal"`
is useful when exact channel values do not matter. Scry's color utility accepts these forms
through one interface, so an application does not need separate settings for names, hex codes,
and channel arrays.

The utility lives in `scry::kit::color`. Use its `Rgb8` and `Rgba8` types directly in config
structs, or choose an adapter when a field should store an ordinary array or tuple. Applications
can also convert the native values into their rendering or imaging library's types.

## Choosing opacity

`Rgb8` stores an opaque sRGB color. `Rgba8` also stores an alpha channel, where 0 is fully
transparent and 255 is fully opaque. A missing alpha channel means 255:

```text
"teal"                   -> [0, 128, 128, 255]
"#2080e0"                -> [32, 128, 224, 255]
[32, 128, 224, 128]       -> [32, 128, 224, 128]
{ r: 32, g: 128, b: 224 } -> [32, 128, 224, 255]
```

Both types accept the same notation. `Rgb8` additionally requires the decoded alpha to be
255, so `"#2080e0ff"` is valid and `"#2080e080"` is an error. Choose the type according to
what the setting means. An opaque canvas should reject translucent input rather than silently
drop its alpha.

Channels are byte values in sRGB. Alpha is separate, or unmultiplied: a transparent red still
stores red channels, even when its alpha is zero. The utility performs no compositing,
premultiplication, or conversion to linear light.

## Accepted forms

| Form | Example | Meaning |
| --- | --- | --- |
| RGB array | `[32, 128, 224]` | Red, green, blue, and an implicit alpha of 255. |
| RGBA array | `[32, 128, 224, 128]` | Red, green, blue, and alpha. |
| Channel map | `{ r: 32, g: 128, b: 224, a: 128 }` | Required `r`, `g`, and `b`, with optional `a`. |
| RGB hex | `"#2080e0"` | Three pairs of hexadecimal digits. |
| RGBA hex | `"#2080e080"` | Four pairs, with alpha last. |
| Short RGB hex | `"#28e"` | Expands to `"#2288ee"`. |
| Short RGBA hex | `"#28e8"` | Expands to `"#2288ee88"`. |
| CSS color name | `"red"`, `"teal"`, `"rebeccapurple"` | A standard CSS named sRGB color. |
| Transparent | `"transparent"` | Transparent black, `[0, 0, 0, 0]`. |

Hex requires a `#` prefix and exactly 3, 4, 6, or 8 digits. Upper- and lowercase digits are
accepted. Short hex duplicates each digit rather than adding a leading zero. Names use the
[CSS named-color vocabulary](https://www.w3.org/TR/css-color-4/#named-colors) and are ASCII
case-insensitive. String parsing ignores surrounding ASCII whitespace.

Structured channels use Scry's native `u8` conversion. Integers must be in `0..=255`, and
decimal integer strings such as `"128"` are also accepted. Floating-point values, negative
values, overflow, and null channels are errors. `[1, 0, 0]` means byte values, regardless of
the destination type. There is no automatic interpretation as normalized floating-point RGB.

Map keys are the literal lowercase names `r`, `g`, `b`, and `a`. Unknown keys are errors.
Omitted `a` becomes 255, while present null is invalid. Arrays require exactly three or four
channels. A color itself cannot be null.

The string language is deliberately small. It does not accept bare hex without `#`, `rgb()`
or HSL functions, percentages, theme references such as `currentColor`, or strings containing
channel arrays. Color matching, color arithmetic, and additional color spaces belong in the
application or its chosen color library.

## Rust values and string parsing

The native types implement `FromStr`, so they can also be used independently of Node decoding:

```rust
use scry::kit::color::{Rgb8, Rgba8};

let background: Rgb8 = " TeAl ".parse()?;
assert_eq!(background.into_array(), [0, 128, 128]);

let overlay: Rgba8 = "#2080e080".parse()?;
assert_eq!(overlay.alpha(), 128);
assert_eq!(overlay.into_array(), [32, 128, 224, 128]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`new` constructs a value from byte channels. Arrays and tuples convert infallibly in either
direction. An RGB color converts into RGBA by adding alpha 255. The reverse conversion checks
opacity:

```rust
use scry::kit::color::{Rgb8, Rgba8};

let rgb = Rgb8::new(32, 128, 224);
let rgba = Rgba8::from(rgb);
assert_eq!(rgba.into_array(), [32, 128, 224, 255]);
assert_eq!(Rgb8::try_from(rgba)?, rgb);

let translucent = Rgba8::from((32, 128, 224, 128));
let error = Rgb8::try_from(translucent).unwrap_err();
assert_eq!(error.alpha(), 128);

let channels: (u8, u8, u8) = rgb.into();
assert_eq!(channels, (32, 128, 224));
# Ok::<(), Box<dyn std::error::Error>>(())
```

`as_array` borrows the channels. `red`, `green`, and `blue` return individual channels, and
`Rgba8::alpha` returns opacity. These are ordinary small values with `Copy`, `Clone`, `Debug`,
`PartialEq`, and `Eq`. Neither type chooses a default color for the application.

### Named colors in Rust

The `named` module provides all 148 opaque CSS named colors as `Rgb8` constants. Names use
the uppercase CSS spelling without inserted underscores, such as `named::TEAL`,
`named::REBECCAPURPLE`, and `named::LIGHTGOLDENRODYELLOW`. Use them when constructing values
or declaring field defaults:

```rust
use scry::kit::color::{named, Rgb8, Rgba8};
use scry::Config;

#[derive(Config)]
struct Accent {
    #[scry(default = named::TEAL)]
    color: Rgb8,
}

let config: Accent = scry::from_defaults()?;
assert_eq!(config.color, named::TEAL);
assert_eq!(named::RED.into_array(), [255, 0, 0]);
assert_eq!(Rgba8::from(named::TEAL).into_array(), [0, 128, 128, 255]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Converting a named constant into `Rgba8` adds alpha 255. The transparent keyword remains a
string accepted by the `Rgba8` parser, since the constants represent opaque colors.

## Scry configuration

Native colors implement `FromNode`, `Describe`, and `ToNode`. They work directly in fields,
including inside ordinary containers:

```rust
use scry::kit::color::{Rgb8, Rgba8};
use scry::node::Format;
use scry::{Config, Node, ToNode};

#[derive(Config, ToNode)]
struct Appearance {
    #[scry(default = Rgb8::new(24, 24, 28))]
    background: Rgb8,
    selection: Rgba8,
    accents: Vec<Rgb8>,
    outline: Option<Rgba8>,
}

let input = r##"#{ selection: "#2080e080", accents: ["teal", [255, 128, 0]] }"##;
let node = Node::parse_str(input, Format::Rhai)?;
let config: Appearance = node.as_type()?;
node.ensure_no_unknown_keys()?;

assert_eq!(config.background.into_array(), [24, 24, 28]);
assert_eq!(config.selection.alpha(), 128);
assert_eq!(config.accents[0].into_array(), [0, 128, 128]);
assert_eq!(config.outline, None);

let output = config.to_node()?;
assert_eq!(output.req::<String>("background")?, "#18181c");
assert_eq!(output.req::<String>("selection")?, "#2080e080");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Missing fields follow the enclosing struct's declared policy. Missing `background` uses its
explicit default, while missing `selection` or `accents` is an error. Missing or null `outline`
becomes `None`. A direct color field rejects null even when it has a fallback. The native color
types have no Rust `Default` or Scry `FromDefaults`.

The examples use Rhai, which needs no format feature flag. Other supported formats use the same
strings, maps, and arrays. JSON requires `format-json`.

### Arrays and tuples

Choose an explicit adapter when the field should keep standard Rust channels:

```rust
use scry::kit::color::{rgb8_array, rgba8_tuple};
use scry::node::Format;
use scry::{Config, Node, ToNode};

#[derive(Config, ToNode)]
struct Appearance {
    #[scry(with(rgb8_array))]
    background: [u8; 3],
    #[scry(with(rgba8_tuple))]
    selection: (u8, u8, u8, u8),
}

let input = r##"#{ background: "teal", selection: "#2080e080" }"##;
let config: Appearance = Node::parse_str(input, Format::Rhai)?.as_type()?;
assert_eq!(config.background, [0, 128, 128]);
assert_eq!(config.selection, (32, 128, 224, 128));
assert_eq!(config.to_node()?.req::<String>("background")?, "#008080");
# Ok::<(), Box<dyn std::error::Error>>(())
```

| Adapter | Destination |
| --- | --- |
| `rgb8_array` | `[u8; 3]`, requiring opaque input. |
| `rgba8_array` | `[u8; 4]`. |
| `rgb8_tuple` | `(u8, u8, u8)`, requiring opaque input. |
| `rgba8_tuple` | `(u8, u8, u8, u8)`. |

Each module exports `from_node`, `to_node`, and `describe`. Its functions reuse the native
color's behavior and can also be called directly. Plain arrays and tuples keep their ordinary
Scry representation unless the field selects an adapter.

An adapter handles its complete field. For `Option<[u8; 4]>` or `Vec<[u8; 4]>`, compose the
color adapter with `scry::convert::read` and `write` helpers for the outer shape. For example,
`read::vec(node, rgba8_array::from_node)` decodes each complete color in a list. Its description
is `Desc::list(rgba8_array::describe())`. Native `Option<Rgba8>` and `Vec<Rgba8>` need no
additional adapter.

## Application color types

An application can keep its imaging or GUI library's type through a local function adapter.
Decode to `Rgb8` or `Rgba8`, convert the channels, and delegate the description and writer.
Scry has no dependencies or optional integrations for foreign color libraries.

The following example uses a small stand-in for a renderer's color type so it runs without
adding a rendering dependency:

```rust
use scry::node::Format;
use scry::{Config, Node, ToNode};

#[derive(Debug, PartialEq)]
struct RendererColor([u8; 4]);

#[derive(Config, ToNode)]
struct Appearance {
    #[scry(with(renderer_color))]
    selection: RendererColor,
}

mod renderer_color {
    use super::RendererColor;
    use scry::kit::color::Rgba8;
    use scry::{Desc, Describe, FromNode, Node, NodeError, ToNode};

    pub fn from_node(node: &Node) -> Result<RendererColor, NodeError> {
        Rgba8::from_node(node).map(|color| RendererColor(color.into_array()))
    }

    pub fn to_node(value: &RendererColor) -> Result<Node, NodeError> {
        Rgba8::from(value.0).to_node()
    }

    pub fn describe() -> Desc {
        Rgba8::describe()
    }
}

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let input = r##"#{ selection: "#2080e080" }"##;
let config: Appearance = Node::parse_str(input, Format::Rhai)?.as_type()?;
assert_eq!(config.selection, RendererColor([32, 128, 224, 128]));
assert_eq!(config.to_node()?.req::<String>("selection")?, "#2080e080");
# Ok(())
# }
```

Replace the channel construction with the destination library's conversion. The application
chooses how that library represents alpha or color space. For an opaque setting stored in an
alpha-capable foreign type, read through `Rgb8` and check opacity again when writing.
Premultiplied destinations can lose channel precision when converted back. Keep the native
value until rendering when exact editable channels matter.

## Writing and diagnostics

`Display` and `ToNode` use canonical lowercase hex. Opaque colors write as `#rrggbb` and
translucent or fully transparent colors write as `#rrggbbaa`:

```rust
use scry::kit::color::{named, Rgba8};

assert_eq!(Rgba8::from(named::TEAL).to_string(), "#008080");
assert_eq!("#28e8".parse::<Rgba8>()?.to_string(), "#2288ee88");
assert_eq!(Rgba8::new(255, 0, 0, 0).to_string(), "#ff000000");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Parsing stores the resolved color rather than the original name, spacing, or input shape.
Saving settings can therefore replace arrays, maps, and names with hex strings. Reading the
output recovers the same native channels, including RGB values at alpha zero.

String parsing returns `ParseError`. Conversion from `Rgba8` to `Rgb8` returns
`NonOpaqueError`, whose `alpha` method identifies the rejected opacity. Scry decoding adds the
configuration path and retains the underlying cause. Invalid structured channels identify their
own location, such as `selection.g` or `accents[2][1]`.

Descriptions explain the alternative input forms as one color value. The current description
model does not expose an untagged string/array/map union, so description-driven editing treats
a color as a whole field. It does not advertise child paths such as `background.r` or
`background[0]`. Raw Node inputs can still have those shapes before decoding.
