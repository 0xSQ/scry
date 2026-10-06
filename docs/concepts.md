# Core Concepts

## The Node Tree

Scry's node tree is roughly what you would expect if you took your typical Rust JSON/TOML/etc. library's internal enum value tree representation and tried to make it input-language-agnostic while making use of Rust's collection of numeric types.

Its basic primitive types are:

- **Bool**
- **Unsigned Integers** (`u8`, `u16`, `u32`, `u64`)
- **Signed Integers** (`i8`, `i16`, `i32`, `i64`)
- **Floats** (`f32`, `f64`)
- **String**
- **Null** (an explicit value, normally decoded as `Option<T>::None`)

### Node Structure

A `Node` is either:

- A **leaf** containing a primitive value (string, number, bool, or null)
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

The two main methods are `req()` for required paths and `opt()` for paths that may be missing:

```rust
// Required value, returns error if missing.
let host: String = node.req("server.host")?;
let port: u16 = node.req("server.port")?;

// Optional path, returns `None` if missing and rejects a present null.
let timeout: Option<u32> = node.opt("server.timeout")?;

// Optional field value, accepts both missing and explicit null as None.
let nullable_timeout: Option<u32> = node.opt::<Option<u32>>("server.timeout")?.flatten();
```

Both methods parse the value into your target type automatically. If the value exists but can't be converted or if your path has invalid syntax, you get an error with helpful details.

Missing and null are distinct. Raw lookup preserves null, while typed lookup lets the target
type decide whether to accept it. See the [`Node` API docs](../scry/src/node.rs) for traversal rules.

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

Derived named structs and named enum payloads reject keys outside their declared fields. Each
decoder checks its own immediate map keys, including keys containing empty objects or arrays.
Child decoders apply their own policies. Earlier reads, failed conversions, and reads through
Node clones do not change which keys the current type permits.

```rust
struct Settings {
    timeout: u32,
}

impl FromNode for Settings {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        let result = Self { timeout: node.req("timeout")? };
        node.ensure_only_keys(&["timeout"])?;
        Ok(result)
    }
}
```

Manual strict map decoders and map hooks own the same responsibility. They can call
`ensure_only_keys` or delegate to a derived strict type. The accepted names are literal map keys.
Unknown-key errors identify the rejected immediate entries, rather than all leaves below them.

To allow extra keys in a particular derived struct:

```rust
use scry::Config;

#[derive(Config)]
#[scry(allow_unknown_keys)]
struct LooseConfig {
    // Extra keys in this object are ignored.
    name: String,
}
```

A strict parent accepts extras inside a declared permissive child. A permissive parent still
delegates its declared child fields to their own decoders, which can be strict.

Scry also retains a separate cumulative unread-input audit:

```rust
let shared_timeout: u32 = node.req("timeout")?;
let shared_name: String = node.req("name")?;
node.ensure_no_unknown_keys()?;
```

This audit recursively reports unread leaves after intentional partial reads. It observes shared
read state, including reads through clones and some failed conversion attempts. Empty containers
have no leaves and are not reported. Use it when the accumulated reads should account for the
whole input. It is separate from structural validation of one configuration type.

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

### Generic Configuration Types

Derives preserve type and const parameters, lifetimes, parameter defaults, and user-written
`where` clauses. Each generated operation adds the bounds needed by the complete field types it
uses. For example, `FromNode` requires `[T; N]: FromNode` for this field, while `Describe` and
`ToNode` require their own corresponding operations:

```rust
use scry::{Config, ToNode};

#[derive(Config, ToNode)]
struct Samples<T = u16, const N: usize = 2> {
    values: [T; N],
}
```

A field of type `T::Value` places its Scry requirement on the associated value type, rather than
requiring the owner `T` to implement that operation. A `*_with` hook replaces only its corresponding
native requirement. Hooks and arbitrary default expressions may need additional constraints that
the caller supplies. An explicit default expression does not require Rust's `Default` trait.
Named `FromDefaults` implementations delegate to the type's `FromNode` implementation. A unit enum
default can be constructed without requiring the enum's payload types to support decoding.

Derives avoid inferring circular bounds for fields that mention `Self`, the unqualified enclosing
type name, or `self::TypeName`. Finite recursive input and output through supported containers such
as `Vec<Self>` use the current implementation and the other field bounds. Use these local spellings
for recursive references because a derive cannot resolve aliases or arbitrary module paths.
Unusual recursive shapes may need caller-written constraints. Descriptions build an ordinary tree,
so recursive descriptions need a custom hook that returns a finite shape. Preserving a lifetime
parameter does not make `FromNode` a borrowing deserializer.

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
- Fields of type `Option<T>` are automatically optional in the input. Without an explicit
  fallback, a missing key becomes `None`. A present null also decodes as `None`.
- Non-optional fields are required unless `#[scry(default = EXPR)]` supplies an explicit value or
  `#[scry(from_defaults)]` recursively applies the field type's Scry defaults.

Fallbacks apply only to missing keys. For example, an `Option<u64>` field with
`#[scry(default = Some(30))]` uses `Some(30)` when missing and `None` when explicitly null.
A present value that cannot be converted is an error, even if the field has a default.

There is no bare `#[scry(default)]` field form. Use an explicit expression even when the
corresponding Rust type also implements `Default`, for example `#[scry(default = Vec::new())]` or
`#[scry(default = OutputMode::Summary)]`. Use `#[scry(from_defaults)]` only for a nested config type
whose own Scry policies should be authoritative. A bare `#[scry(default)]` does have a separate,
deliberate meaning on a unit enum variant, as described below.

Descriptions automatically show only simple literal defaults such as `false`, `3`, `-0.5`, and
`"cache"`. Constructor calls, enum paths, constants, and other Rust expressions still make the
field omittable, but `--desc` does not present their source text as if it were a config value.
The description's optional marker means a field can be omitted, not that it accepts null.

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

A field name is always one literal map key. For example, `#[scry(rename = "server.port")]`
matches `{ "server.port": 9000 }`. It does not navigate into `{ "server": { "port": 9000 } }`.
Use a nested config type to represent nested objects. Ordinary Node and CLI query paths still
use path syntax, so `["server.port"]` selects the literal dotted key and `server.port` selects
the nested value.

Derived named objects reject duplicate effective field keys. A renamed field must not collide
with another renamed or ordinary field. Enum variant names must also be unique, and their
accepted spellings must not overlap within the same input form.

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

For data-carrying enums, `.variant(key)` wraps the supplied value in the enum's single-key map
and replaces the whole field. This lets selecting a variant displace the previous one, instead
of adding a second variant key through `--set`.

```rust
Setup::standard("app")
    .expose(|e| {
        e.option("output").variant("file");  // --file <VALUE>  ->  output: { "file": "<VALUE>" }
        e.option("output").variant("remote").array(2).value_names(["HOST", "PORT"]);
    })
```

Here, `--remote logs.example.com 5140` supplies the tuple's two elements. They enter the tree as
strings and are converted to the tuple's Rust types afterward.

More generally, `.array(arity)` groups CLI values into an array, while `.append()` extends a
configured collection instead of replacing it. The [`ExposeMap` API docs](../scry/src/cli/setup/expose_map.rs)
cover combinations and parsing boundaries.

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
            node.ensure_only_keys(&["side"])?;
            (side, side)
        } else {
            let dimensions = (node.req("width")?, node.req("height")?);
            node.ensure_only_keys(&["width", "height"])?;
            dimensions
        };

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

The hook returns the complete field type, including `Option<T>`. Missing fields use their
fallback without calling it. See the [derive docs](../scry_derive/src/lib.rs) for the contract.

Input errors use the full logical path already carried by the `Node`. `Node::as_type` and derived
field hooks attach that path to errors without a logical location. They preserve existing error
paths, so enclosing decoders do not repeat field names or indices.

When a domain parser returns an error, keep both its original cause and the input location:

```rust
let value = parse_domain_value(&text).map_err(|error| {
    NodeError::invalid_value_with_source(&node.path, "invalid domain value", error)
})?;
```

`NodeError::path()` exposes a single logical configuration location. `Error::source()` retains the
original concrete cause and its own error chain. Primitive string conversions also preserve their
integer, float, or boolean parser errors. A locationless filesystem or format error can carry an
enclosing configuration location without changing its original filesystem path or cause.

### Hooks on Positional Fields

The three field hooks also work on transparent newtypes, tuple structs, and positional enum
payloads. Their functions receive or return the complete field type, including `Option<T>`:

```rust
use scry::{FromNode, Node, NodeError};

#[derive(FromNode)]
struct Positive(#[scry(from_node_with(read_positive))] u32);

fn read_positive(node: &Node) -> Result<u32, NodeError> {
    let value = node.as_type::<u32>()?;
    if value == 0 {
        return Err(NodeError::invalid_value(&node.path, "must be positive"));
    }
    Ok(value)
}
```

`Positive` still decodes directly from a scalar. A multi-field tuple still requires an array of
exactly its declared length, and an enum payload retains its variant key. Positional hooks use
those same input locations and serialization error prefixes as native operations.

Positional fields reject `rename`, `default = EXPR`, and `from_defaults`. They have no named key to
rename or omit. A present null still goes through the complete field's parser or hook.

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
omitted `server` report `missing value for 'server.host'`. Only a missing field invokes recursive
defaults. An explicit null is decoded as the field's type and is invalid for `ServerConfig`.
An explicit empty map instead decodes the child and applies the child's own field policies.

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

`None` fields serialize as null so reading them back preserves `None` instead of restoring a
default. This requires a format that supports null, such as JSON or Rhai. TOML rejects it.

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
            let side = self.width.to_node()
                .map_err(|error| error.prepend_path(&KeyPath::from_keys(["side"])))?;
            map.insert("side".to_string(), side);
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

#[derive(Config, ToNode)]
struct Theme {
    #[scry(from_node_with(parse_color), to_node_with(color_to_node))]
    background: Color,
}

fn color_to_node(color: &Color) -> Result<Node, NodeError> {
    color.to_hex_string().to_node()
}
```

Like the parsing hook, the serializer handles the complete field type. For an optional color,
it takes `&Option<Color>`, including `None`.

Serialization errors carry paths relative to the value being serialized. A leaf serializer can
report an empty path with `NodeError::invalid_value_with_source(&KeyPath::new(), message, cause)`.
Each enclosing serializer prepends the exact key or index through which it called that child, using
`NodeError::prepend_path`. Existing relative segments and original causes remain intact.

For example, if the second job in `jobs: Vec<Job>` fails while serializing its `limit` field, the
error identifies `jobs[1].limit`. Derived named fields, hooks, tuples, arrays, and enum payloads add
their serialized locations automatically. Renamed keys remain literal. Transparent newtypes,
`Option`, and references preserve the child's relative path. `KeyValues` emits a list of pairs, so
a value failure uses its output position such as `[1][1]`, including when keys repeat.

Manual structured serializers apply the same rule, as shown for the `side` key above. Input decoding
uses `NodeError::at_path` to attach only a missing location because input paths are already complete.
Successful output Nodes can still have empty paths. `Node::set_node` anchors their subtrees at the
destination as before.

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

The first paragraph of a type's documentation becomes the root description. Named fields keep
their own prose separately from the value's type description. A positional field's nonempty prose
overrides its delegated value prose. On a transparent newtype, nonempty type prose takes priority
over positional field prose, which takes priority over the inner type or hook's prose. An absent
override preserves the delegated prose. Enum variant prose and positional payload prose remain
separate.

Descriptions compose through the complete Rust type. `Vec<Vec<u32>>` and a type alias for it both
produce `list[list[u32]]`, and paths such as `samples[0][1]` can select the inner element description.
References, `Box`, `Rc`, and `Arc` forward the inner description. Raw `Node` fields have an opaque
`value | null` description because their contents can have any shape.

Nullability belongs to the value description and is separate from field omission:

```rust
use scry::Describe;

#[derive(Describe)]
struct Samples {
    values: Vec<Option<u32>>,
    limit: Option<u32>,
    #[scry(default = 0)]
    retries: u32,
}
```

The resulting field labels are:

```text
◆ values: list[u32 | null]
◇ limit: u32 | null
◇ retries: u32 → 0
```

`values` is required and its elements accept null. `limit` can be omitted and accepts a present null.
`retries` can be omitted but rejects a present null. `FieldDesc.optional` records omission, while
`Desc.nullable` records acceptance of null alongside the described shape. A nullable structured
value retains its children and enum choices. Repeated `Option` layers share one nullable shape.
This does not change how missing keys select defaults. A type alias hiding `Option<T>` still needs
an explicit fallback to permit omission.

Every field described by a derive needs `Describe` or a `describe_with` hook. A missing
implementation is a compiler error. Use a labelled plain description for an intentionally opaque
custom value.

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

The hook describes the complete field value and replaces native delegation. For a nullable custom
value, return a description such as `Desc::plain("hex color").nullable()`. The derive does not infer
nullability or container shape for a custom hook. Field documentation, omission policy, and displayed
defaults still apply separately.
