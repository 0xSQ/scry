# Writing documented configuration files

A settings file is easier to edit when the explanations live beside the values. Scry can
generate those comments along with the configuration, whether you are saving an application's
current preferences or giving users a starting file to fill in.

The examples below build up an image viewer's preferences file. First we construct a
document by hand, then reuse a configuration type's documentation, and finally generate a
template before any preferences have been chosen.

`Document` combines a language-neutral `Node` tree with comments and presentation hints.
`RhaiWriter` renders Rhai, while `Json5Writer` renders JSON5 when the `format-json5` feature
is enabled (as it is by default). The same document can be rendered in either format.

## Constructing a document

Start with a few values and the explanations a user needs to edit them. Map and sequence
builders retain insertion order. Comments attach to entries or elements, so the writer can
place commas, indentation, and comment markers correctly:

```rust
use scry::writer::{Document, RhaiWriter};

let document = Document::map(|map| {
    map.entry("background", "#1c1c1e")?
        .comment("Canvas color behind transparent pixels.");
    map.entry("zoom_factor", 1.2)?
        .blank_before()
        .comment("Multiplier per zoom step.")
        .end_comment("20% per step.");
    map.entry_with("favorites", |value| {
        value.seq(|sequence| {
            sequence.elem("landscapes")?;
            sequence.elem("portraits")?;
            Ok(())
        })
    })?;
    Ok(())
})?
.header("Viewer preferences.");

let mut writer = RhaiWriter::to_string_writer();
writer.write(&document)?;
let text = writer.into_string()?;
```

The relevant part of the generated Rhai is:

```rhai
// Viewer preferences.
#{
    // Canvas color behind transparent pixels.
    background: "#1c1c1e",

    // Multiplier per zoom step.
    zoom_factor: 1.2, // 20% per step.
    favorites: ["landscapes", "portraits"],
}
```

Pass comment text without `//` markers. `comment`, `header`, `footer`, and `tail_comment`
accept multiple lines. `end_comment` requires a single line. `tail_comment` adds text
inside a map or sequence before its closing delimiter and returns an error for a scalar.
`footer` adds text after the root value. `entry_document` and `elem_document` insert
separately constructed documents. `entry_with` and `elem_with` build nested values inline.

Use `compact()` or `expanded()` on a document or entry handle to request a container's
layout. Comments requiring separate lines cause the affected container to expand.
`at_mut("favorites")` returns an annotation handle for an existing path.
`replace("zoom_factor", 1.3)` changes an existing value while retaining that entry's
comments and layout. Replacing a container discards annotations inside the old container.

## Writing from a configuration value

If the viewer already has a configuration struct, `Document::from_config` creates a document
from its current values and field documentation. Derive `ToNode` alongside `Config` to make
the settings writable:

```rust
use std::path::PathBuf;

use scry::{Config, ToNode};
use scry::writer::Document;

#[derive(Config, ToNode)]
struct Preferences {
    /// Canvas color behind transparent pixels.
    background: String,
    /// Multiplier per zoom step.
    #[scry(default = 1.2)]
    zoom_factor: f64,
    /// Budget for retained image data, in MiB.
    #[scry(default = 512)]
    cache_size_mib: usize,
    /// Directory for copied images.
    target_dir: Option<PathBuf>,
}

let preferences = Preferences {
    background: "#1c1c1e".to_owned(),
    zoom_factor: 1.1,
    cache_size_mib: 256,
    target_dir: None,
};
let document = Document::from_config(&preferences)?;
```

Pass this document to a writer as before. The output contains the selected values `1.1` and
`256`, rather than the defaults `1.2` and `512`. Described struct fields use declaration
order. Other map entries retain their supplied order. The derive uses the first paragraph
of each doc comment.

`from_config` always needs a complete value. If every field has a missing-value fallback,
`scry::from_defaults::<T>()?` can construct one to use as a starting point. Here, the
application must also supply `background`. To leave that choice to the user, generate a
template from the type instead.

Use `scry::kit::KeyValues<T>` for associative configuration fields. It already supports
reading, writing, and descriptions. It enforces unique string keys, and its canonical
list-of-pairs output preserves entry order. Use `Document::map` when constructing a
literal object instead.

Custom types can supply `#[scry(to_node_with(write_value))]` on a field, alongside matching
read and description hooks. For example, a GUI color can serialize as a hex string through
one adapter shared by every writer. See the runnable
[`documented_settings`](../scry/examples/documented_settings.rs) example for color adapters,
nested settings, relative paths, and parse/decode checks:

```sh
cargo run --example documented_settings
```

## Generating a template from the type

A new user may need a starting file before they have chosen any settings. `format_template`
generates a scaffold from the configuration type without requiring a complete `Preferences`
value or a `ToNode` implementation:

```rust
use scry::node::Format;
use scry::writer::{format_template, TemplateMode};

let text = format_template::<Preferences>(Format::Json5, TemplateMode::Ref)?;
```

For the type above, this produces:

```json5
// Fill required placeholders and uncomment their entries.
// Commented optional entries may be enabled when needed.
{
    // Canvas color behind transparent pixels. Required.
    // "background": <string>,

    // Multiplier per zoom step. Optional.
    // "zoom_factor": 1.2, // type: f64

    // Budget for retained image data, in MiB. Optional.
    // "cache_size_mib": 512, // type: usize

    // Directory for copied images. Optional. Omitted by default.
    // "target_dir": <path>,
}
```

Unlike `from_config`, this starts from the type's defaults and leaves unknown values as
commented placeholders. Replace `<string>` with a quoted color such as `"#1c1c1e"` and
uncomment the `background` entry. The other keys may stay commented to use their fallbacks.
`Optional.` means the key may be omitted, whether it has a default or accepts an absent value.
Concrete scalar defaults have an inline type label. Typed placeholders explain themselves.

### Choosing how much to include

The template mode controls which fields appear and whether known defaults are active:

| Mode | Generated content |
| --- | --- |
| `min` (default) | Only required fields and their enclosing objects. |
| `ref` | Required fields, commented defaults, and optional structures to explore. |
| `explicit` | Known defaults as active values, including `()` in Rhai or `null` in JSON5 for absent options. |

For `Preferences`, `min` shows just the `background` placeholder. `explicit` also writes
`zoom_factor: 1.2`, `cache_size_mib: 512`, and `target_dir: null` in JSON5. Unknown values
remain commented placeholders in every mode. A syntactically valid template may still need
application-specific settings before it can run.

Commands built with `Setup` expose this through `-S template`. They write to stdout without
loading an existing configuration or running the payload. Rhai is the default output
language. `--as` selects the language independently of `--mode`:

```sh
viewer -S template
viewer -S template --as json5 --mode ref
viewer -S template --as json5 --mode explicit > preferences.json5
```

The last command saves a file for editing. The command itself does not install the file or
choose where the application will look for it. See [Config File Locations](config-files.md)
for file selection and discovery. For a runnable example with required fields, nested
objects, and optional features, try
[`config_template`](../scry/examples/config_template.rs):

```sh
cargo run --example config_template -- -S template --mode ref
```

### Editing nested structures

Optional objects in `ref` mode show their children inside a commented container. Each line
has one leading `//` at its natural indentation. Enable the container's opening and closing
lines, then enable the children you need individually. Keep explanatory comments intact.
Uncommenting an entire block would also turn its explanatory prose into code.

Lists show one commented element to fill in before enabling it. For a fixed-size array,
add the remaining elements yourself. Custom input types may show only a type hint rather
than examples of all their accepted forms.

### Template limitations

Some defaults, particularly for custom or adapted types, cannot be written automatically.
They are labelled unavailable and shown as commented placeholders. Leave these entries
commented to use their defaults, or supply values yourself.

Templates currently support an object at the root. If a custom loader expects a named Rhai
variable or an enclosing JSON5 member, add that wrapper to the generated file. Enum variants
with payloads may need to be filled in by hand, and deeply nested structures may be abbreviated.

## Choosing output and layout

For documents built by hand or with `from_config`, the writer controls the final layout.
Both writers accept any `std::io::Write` destination through `new(output)` or
`with_config(output, config)`. `RhaiWriterConfig` and `Json5WriterConfig` expose `indent`,
`trailing_commas`, and `inline_seq_threshold`. Defaults are four spaces, trailing commas,
and inline sequences of at most two simple elements. Per-container hints override automatic
layout where comments permit it.

`write(&document)` writes annotations and a final newline. `value(value)` accepts `ToNode`
and writes an unannotated value without a final newline. `to_string_writer()` collects
output in memory. `as_str()` borrows it and `into_string()` extracts the result.

`Json5Writer` uses `{ ... }` maps and `null`, while Rhai uses `#{ ... }` and `()`.
Both writers reject nonfinite numbers. Rhai uses signed 64-bit integers. JSON5 output
currently uses that same integer range to stay readable by Scry's existing JSON5 parser.
An unsupported value reports its configuration path before any document text is written.
For strict JSON, explicitly discard annotations and use the ordinary value writer:

```rust
use scry::node::Format;

let json = document.into_node().to_string_as(Format::Json)?;
```

## Saving application preferences

Keep editable preferences separate from runtime state. For example, retain relative paths
in preferences and resolve them separately when applying settings. A save action can
validate the edited preferences, build `Document::from_config(&preferences)`, render it
in memory, and then persist the complete file using the application's file-write policy.
This saves the user's current choices. A template is useful earlier, when those choices
have yet to be made.

These writers generate fresh documents. Parsing an existing file into `Node` discards its
comments, formatting, and Rhai expressions. Regenerating that file cannot preserve personal
notes added by hand. An application offering GUI saves should make this ownership clear.
Reading annotated documents or preserving authored source is outside this facility's scope.
