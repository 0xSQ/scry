# Core Concepts

## The Node Tree

Scry's node tree is roughly what you would expect if you took your typical Rust JSON/TOML/etc. library's internal enum value tree representation and tried to make it input-language-agnostic while making use of Rust's collection of numeric types.

Its basic primitive types are:

- **Bool**
- **Unsigned Integers** (`u8`, `u16`, `u32`, `u64`)
- **Signed Integers** (`i8`, `i16`, `i32`, `i64`)
- **Floats** (`f32`, `f64`)
- **String**
- **Null** (in rare cases when we need to represent `None` explicitly)

### Node Structure

A `Node` is either:

- A **leaf** containing a primitive value (string, number, bool)
- A **vec** containing an ordered list of child nodes
- A **map** containing named child nodes

Each node tracks its path in the tree (e.g., `"server.tls.cert_file"`) for error messages.

### Creating Nodes

From a file (format detected by extension):

```rust
let node = Node::parse_file("config.json")?; // Requires `format-json` (enabled by default).
let node = Node::parse_file("config.json5")?; // Requires `format-json5` (enabled by default).
let node = Node::parse_file("config.rhai")?;
```
Note: TOML and YAML support are optionally available via `format-toml` and `format-yaml`.

From a string with explicit format:

```rust
use scry::node::Format;

let node = Node::parse_str(json_string, Format::Json)?;
let node = Node::parse_str(rhai_script, Format::Rhai)?;
```

From a Rhai `Dynamic` value (useful with custom Rhai engines):

```rust
let node = Node::from_rhai_dynamic(dynamic_value)?;
```

Formats are resolved through registries, and custom formats can be registered for both parsing
and writing via `FormatParserRegistryBuilder` and `FormatWriterRegistryBuilder`. For in-depth usage and
examples, see the API docs on `Format`, `ConfigFormatParser`, `ConfigFormatWriter`, and registry builders.

### Reading Values

The two main methods are `req()` for required values and `opt()` for optional ones:

```rust
// Required value, returns error if missing.
let host: String = node.req("server.host")?;
let port: u16 = node.req("server.port")?;

// Optional value, returns `None` if missing.
let timeout: Option<u32> = node.opt("server.timeout")?;
```

Both methods parse the value into your target type automatically. If the value exists but can't be converted or if your path has invalid syntax, you get an error with helpful details.

For navigating to a subsection without parsing:

```rust
let server_node = node.req_node("server")?;
let tls_node = node.opt_node("server.tls")?;  // Returns Option<&Node>
```

### Path Syntax

Paths use dot notation for nested keys and brackets for array indices:

| Path                | Meaning                                                  |
| ------------------- | -------------------------------------------------------- |
| `"host"`            | Top-level key                                            |
| `"server.host"`     | Nested key                                               |
| `"servers[0]"`      | First array element                                      |
| `"servers[0].host"` | Key within array element                                 |
| `["server.host"]`   | Literal key containing a dot or other special characters |

### Modifying Values

Nodes are mutable. You can override values before converting to your struct:

```rust
let mut node = Node::parse_file("config.json")?;
node.set_value("server.port", 9000)?;
node.set_value("server.verbose", true)?;
node.remove("server.legacy_option")?;

let config: ServerConfig = node.as_type()?;
```

### Serialization

Nodes can be serialized back to JSON or Rhai:

```rust
use scry::node::Format;

let json_string = node.to_string_as(Format::Json)?;
let rhai_string = node.to_string_as(Format::Rhai)?;
```

### Unknown Key Detection

By default, Scry tracks which keys are actually read from a node. If any keys remain unread after parsing, `ensure_no_unknown_keys()` will error:

```rust
let config: MyConfig = node.as_type()?;
node.ensure_no_unknown_keys()?;  // Error if config.json had keys we didn't read
```

Call `ensure_no_unknown_keys()` when the node represents the complete definition of your type. This catches typos and stale keys. Skip it when your type intentionally reads only some keys from a larger structure (e.g., reading shared settings from a config that other types also read from).

When using derive macros, this check is included by default. To disable it for a particular struct:

```rust
use scry::Config;

#[derive(Config)]
#[scry(allow_unknown_keys)]
struct LooseConfig {
    // Only these fields are read; extra keys are ignored
    name: String,
}
```

## The Core Traits

Scry defines four traits for working with configuration types:

| Trait / Derive Macro | Purpose                                                  |
| -------------------- | -------------------------------------------------------- |
| `FromNode`           | Parse a `Node` into a Rust type                          |
| `FromDefaults`       | Construct a config type from its Scry field policies     |
| `ToNode`             | Serialize a Rust type to a `Node`                        |
| `Describe`           | Generate type descriptions for documentation             |

The `#[derive(Config)]` macro is a shorthand for deriving the most common combination of
`FromNode` and `Describe` together. For named structs it also derives `FromDefaults`. An enum gets
`FromDefaults` when exactly one unit variant has `#[scry(default)]`. You can derive the traits
individually when you only need some functionality. On enums, `Config` can also generate Rust
string conversion when requested with `#[scry(from_str)]`.

`FromNode`, `ToNode`, and `Describe` have corresponding `*_with` field attributes for customizing
individual fields without implementing the full trait. `FromDefaults` is selected explicitly with
the `#[scry(from_defaults)]` field policy described below.

## FromNode

`FromNode` defines how to parse a `Node` into a Rust type:

```rust
pub trait FromNode: Sized {
    fn from_node(node: &Node) -> Result<Self, NodeError>;
}
```

Scry implements this for common types:

- **Scalars**: `bool`, integers (`i8`..`i64`, `u8`..`u64`), floats (`f32`, `f64`), `String`, `PathBuf`
- **Containers**: `Option<T>`, `Vec<T>`, `[T; N]`, tuples (up to 4 elements)
- **Composite**: structs, enums, tuple structs

### Deriving FromNode

```rust
use scry::FromNode;

#[derive(FromNode)]
struct ServerConfig {
    host: String,
    port: u16,
    #[scry(default = 100)]
    max_connections: u32,
}
```

### Structs

Structs have a straightforward default implementation:

- Each field is read from a map key with the same name as the field.
- Fields of type `Option<T>` are automatically optional in the input. If the key is missing, the field is set to `None`.
- Non-optional fields are required unless `#[scry(default = EXPR)]` supplies an explicit value or
  `#[scry(from_defaults)]` recursively applies the field type's Scry defaults.

There is no bare `#[scry(default)]` field form. Use an explicit expression even when the
corresponding Rust type also implements `Default`, for example `#[scry(default = Vec::new())]` or
`#[scry(default = OutputMode::Summary)]`. Use `#[scry(from_defaults)]` only for a nested config type
whose own Scry policies should be authoritative. A bare `#[scry(default)]` does have a separate,
deliberate meaning on a unit enum variant, as described below.

Descriptions automatically show only simple literal defaults such as `false`, `3`, `-0.5`, and
`"cache"`. Constructor calls, enum paths, constants, and other Rust expressions still make the
field omittable, but `--desc` does not present their source text as if it were a config value.

### The `rename` Attribute

Use `#[scry(rename = "...")]` to change the config key name for a field:

```rust
#[derive(Config)]
struct DatabaseConfig {
    #[scry(rename = "host")]
    hostname: String,
    #[scry(rename = "db")]
    database_name: String,
}
```

This struct expects `{ "host": "...", "db": "..." }` in the config, but uses `hostname` and `database_name` as Rust field names. The rename applies throughout the generated config behavior.

### Enums

Scry supports all Rust enum variant types: unit, tuple, and struct. For unit variants, the variant name as a string is used. For tuple and struct variants, a map with the variant name as the key is used. Single-field "newtype" tuple variants are special-cased to allow direct value representation without an extra array:

| Variant Type       | Config Format                       |
| ------------------ | ----------------------------------- |
| Unit               | `"name"`                            |
| Single-field tuple | `{ "name": value }`                 |
| Multi-field tuple  | `{ "name": [v1, v2, ...] }`         |
| Struct             | `{ "name": {"field": value, ...} }` |

Variant names are translated from Rust's `PascalCase` to `snake_case` by default. Matching accepts both `snake_case` and `kebab-case` spellings for compound variant names, and unit variant matching is case-insensitive. Struct variant fields support the same attributes as regular struct fields (`#[scry(default = EXPR)]`, `#[scry(from_defaults)]`, `#[scry(rename)]`, etc.).

An enum can declare its Scry-owned default by marking exactly one unit variant:

```rust
#[derive(Config)]
enum OutputMode {
    #[scry(default)]
    Summary,
    Full,
}
```

This gives `OutputMode` a `FromDefaults` implementation. A field opts into it with
`#[scry(from_defaults)]`. The marker is unrelated to Rust's `#[default]`, so an enum may choose
different variants for Scry construction and `std::default::Default`. Payload variants cannot be
Scry defaults initially.

Descriptions show the `»` marker when the enum itself is being constructed from Scry defaults,
including a direct enum field with `#[scry(from_defaults)]`. Required fields and fields with an
explicit `default = EXPR` do not show that marker because their missing-value policies do not select
the enum's type-level Scry default.

Use `#[scry(rename_all = "kebab-case")]` on an enum when you want kebab-case to become the
canonical config and description spelling instead:

```rust
#[derive(Config)]
#[scry(rename_all = "kebab-case")]
enum ProcessOrder {
    RowMajor,
    ColMajor,
}
```

### Enum String Conversion

You do not need any extra attributes just to use an enum in config. `#[derive(Config)]`
already teaches Scry how to read enum variants from a `Node`, how to describe them for
`--desc`, and how to expose unit enums as CLI possible values.

Add `#[scry(from_str)]` only when your Rust code also wants ordinary string conversion:

```rust
use scry::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Config)]
#[scry(from_str)]
enum OutputFormat {
    Summary,
    Raw,
}
```

This generates both `std::str::FromStr` and `std::fmt::Display` using the same canonical
variant spelling that Scry uses for config. In this example:

```rust
assert_eq!(OutputFormat::Summary.to_string(), "summary");
assert_eq!("raw".parse::<OutputFormat>().unwrap(), OutputFormat::Raw);
```

The generated impls respect `#[scry(rename = "...")]` on variants and
`#[scry(rename_all = "kebab-case")]` on enums. Parsing is case-insensitive for unit variants
and accepts snake-case/kebab-case aliases for compound names, matching Scry's config parser.

Do not add `#[scry(from_str)]` if you want to write your own `FromStr` or `Display`
implementation for the enum. Rust allows only one impl of each trait for a type, so the
generated impls would conflict with your manual ones.

`StringEnum` is the standalone derive behind this behavior. Most config types should use
`#[derive(Config)]` with `#[scry(from_str)]`; derive `StringEnum` directly only for a unit enum
that is not a config type but should still use Scry's variant spelling rules:

```rust
use scry::StringEnum;

#[derive(Debug, Clone, Copy, StringEnum)]
#[scry(rename_all = "kebab-case")]
enum SortOrder {
    RowMajor,
    ColMajor,
}
```

Example:

```rust
use scry::Config;

#[derive(Config)]
enum Output {
    Stdout,
    File(String),
    Remote(String, u16),
    Database { host: String, table: String },
}
```

The four variants can be read as:

```json
"stdout"
```

```json
{ "file": "/var/log/app.log" }
```

```json
{ "remote": ["logs.example.com", 5140] }
```

```json
{ "database": { "host": "db.example.com", "table": "logs" } }
```

### Enums on the Command Line

For unit enums, exposing the field as a plain option is enough: `#[derive(Config)]` already
teaches the CLI layer the variant names, and the exposed option gets them as possible values.

Data-carrying enums need one more piece. Their single-key-map form interacts poorly with `--set`:
`--set` values are plain string leaves, and a dotted path like `--set output.file app.log` grafts
a `file` key into whatever map is already there - if the config held `{ "remote": ... }`, the
result is a two-key map that enum parsing rejects as ambiguous. The `variant` modifier on exposed
entries closes this gap. It declares that the entry addresses one variant of the enum field: the
entry's value is wrapped as `{ "<key>": value }` and assigned at the field's path wholesale, so
selecting one variant always displaces whichever variant the config held.

```rust
Setup::standard("app")
    .expose(|e| {
        e.option("output").variant("file");  // --file <VALUE>  ->  output: { "file": "<VALUE>" }
    })
```

- Without a custom long name (`.long(...)`), the option name derives from the variant key rather
  than the field path - `--file` above.
- The CLI value is still one string, so an exposed option reaches variants whose payload parses
  from a single string (`file` here). Variants with structured payloads (`remote`, `database`)
  stay config-only - or use a `flag`, whose fixed `ToNode` value is wrapped the same way and may
  be structured.
- Pass the *serialized* variant key, i.e. the spelling after `rename` / `rename_all`.
- The modifier composes with `option`, `positional`, and `flag`; `list` entries reject it.
  Variant options participate in the usual command-line override order, later operations winning.

More generally, the expose vocabulary splits along two axes: constructors (`option`, `flag`,
`list`, `positional`) name an argument's CLI shape, while modifiers like `variant` refine its
presentation and write shape.

### Implementing FromNode Manually

For custom parsing logic (multiple input formats, validation, computed fields), implement `FromNode` yourself. This example accepts three different input formats:

```rust
use scry::{FromNode, Node, NodeError};

struct Rectangle {
    width: u32,
    height: u32,
    area: u32,
}

impl FromNode for Rectangle {
    /// Parses a Rectangle from one of three formats:
    /// - `[800, 600]`: width and height array
    /// - `{ side: 512 }`: square shorthand
    /// - `{ width: 800, height: 600 }`: explicit form
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        let (width, height) = if let Some(arr) = node.as_opt_vec() {
            if arr.len() != 2 {
                return Err(NodeError::array_length(&node.path, 2, arr.len()));
            }
            (arr[0].as_type()?, arr[1].as_type()?)
        } else if let Some(side) = node.opt::<u32>("side")? {
            (side, side)
        } else {
            (node.req("width")?, node.req("height")?)
        };

        node.ensure_no_unknown_keys()?;

        if width == 0 {
            return Err(NodeError::invalid_value(&node.path, "width must be positive"));
        }
        if height == 0 {
            return Err(NodeError::invalid_value(&node.path, "height must be positive"));
        }

        Ok(Self { width, height, area: width * height })
    }
}
```

### The `from_node_with` Attribute

For types you don't own (external crates), you can't implement `FromNode` due to Rust's orphan rules. Use `from_node_with` to specify a custom parsing function for individual fields:

```rust
use scry::{Config, Node, NodeError};
use some_crate::Color;

#[derive(Config)]
struct Theme {
    #[scry(from_node_with(parse_color))]
    background: Color,
}

fn parse_color(node: &Node) -> Result<Color, NodeError> {
    let hex: String = node.as_type()?;
    Color::from_hex(&hex)
        .ok_or_else(|| NodeError::invalid_value(&node.path, format!("invalid hex color: {hex}")))
}
```

If you use the same external type in many places, consider creating a newtype wrapper with its own `FromNode` implementation instead.

## FromDefaults

`FromDefaults` constructs a config value by applying its Scry field policies at a logical config
path:

```rust
pub trait FromDefaults: Sized {
    fn from_defaults_at(path: &KeyPath) -> Result<Self, NodeError>;
}
```

`Config` derives this trait for named structs. The generated implementation parses an empty map
through the same `FromNode` implementation used for authored config, so there is only one field
interpreter. For enums, `Config` generates `FromDefaults` when exactly one unit variant has
`#[scry(default)]`. The path anchors diagnostics and must not influence the value being constructed.

Use `#[scry(from_defaults)]` when omitting a nested field should recursively apply that type's own
Scry policies:

```rust
use scry::Config;

#[derive(Config)]
struct AppConfig {
    #[scry(from_defaults)]
    server: ServerConfig,
}

#[derive(Config)]
struct ServerConfig {
    #[scry(default = "127.0.0.1".to_string())]
    host: String,
    #[scry(default = 8080)]
    port: u16,
}

let config: AppConfig = scry::from_defaults()?;
```

A required descendant remains an error. For example, removing the `host` default above makes an
omitted `server` report `missing value for 'server.host'`. Missing fields and explicit `null` values
both invoke the selected field fallback, matching Scry's existing optional-value behavior.

Use the standalone `FromDefaults` derive alongside `FromNode` when you do not want the complete
`Config` bundle for a named struct. The standalone derive also supports enums with exactly one unit
variant marked `#[scry(default)]`. Tuple structs should use explicit field expressions or a manual
implementation where appropriate.

## ToNode

`ToNode` converts a Rust type back into a `Node` tree for serialization:

```rust
pub trait ToNode {
    fn to_node(&self) -> Result<Node, NodeError>;
}
```

### Deriving ToNode

```rust
use scry::ToNode;

#[derive(ToNode)]
struct Output {
    status: String,
    code: u32,
}

let output = Output { status: "ok".into(), code: 200 };
let node = output.to_node()?;
let json = node.to_string_as(scry::node::Format::Json)?;
```

### Implementing ToNode Manually

For custom serialization (compact formats, omitting computed fields), implement `ToNode` yourself:

```rust
use indexmap::IndexMap;
use scry::{KeyPath, Node, NodeError, ToNode};

struct Rectangle {
    width: u32,
    height: u32,
    area: u32,
}

impl ToNode for Rectangle {
    /// Serializes to the most compact form:
    /// - Squares become `{ side: n }`
    /// - Non-squares become `[width, height]`
    fn to_node(&self) -> Result<Node, NodeError> {
        if self.width == self.height {
            let mut map = IndexMap::new();
            map.insert("side".to_string(), self.width.to_node()?);
            Ok(Node::new_map(KeyPath::default(), map))
        } else {
            (self.width, self.height).to_node()
        }
    }
}
```

### The `to_node_with` Attribute

Use `to_node_with` to specify a custom serialization function for individual fields:

```rust
use scry::{Config, Node, NodeError, ToNode};
use some_crate::Color;

#[derive(Config)]
struct Theme {
    #[scry(from_node_with(parse_color), to_node_with(color_to_node))]
    background: Color,
}

fn color_to_node(color: &Color) -> Result<Node, NodeError> {
    color.to_hex_string().to_node()
}
```

## Describe

`Describe` generates type descriptions for documentation (used by the CLI `--desc` flag):

```rust
pub trait Describe {
    fn describe() -> Desc;
}
```

### Deriving Describe

```rust
use scry::Describe;

#[derive(Describe)]
struct ServerConfig {
    /// The server hostname.
    host: String,
    /// Port to listen on.
    port: u16,
}

println!("{}", ServerConfig::describe().display());
```

Doc comments on fields become descriptions in the output.

### Implementing Describe Manually

For simple types, return a plain description:

```rust
use scry::{Desc, Describe};

struct Rectangle {
    width: u32,
    height: u32,
    area: u32,
}

impl Describe for Rectangle {
    fn describe() -> Desc {
        Desc::plain("rectangle")
    }
}
```

### The `describe_with` Attribute

Use `describe_with` to specify a custom description function for individual fields:

```rust
use scry::{Config, Desc};
use some_crate::Color;

#[derive(Config)]
struct Theme {
    #[scry(from_node_with(parse_color), describe_with(color_desc))]
    background: Color,
}

fn color_desc() -> Desc {
    Desc::plain("hex color")
}
```
