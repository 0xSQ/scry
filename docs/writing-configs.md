# Writing documented configuration files

`Document` combines a language-neutral `Node` tree with comments and presentation hints.
Use it to generate readable settings files, documented examples, or ordinary data files.
`RhaiWriter` renders Rhai, while `Json5Writer` renders JSON5 when the `format-json5` feature
is enabled. The same document can be rendered in either format.

The examples below use an image viewer's settings as a motivating example.

## Constructing a document

Map and sequence builders retain insertion order. Comments attach to entries or elements,
so the writer can place commas, indentation, and comment markers correctly:

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

## Using configuration metadata

`Document::from_config` combines actual values from `ToNode` with comments from `Describe`.
Derive `ToNode` separately alongside `Config`:

```rust
use scry::{Config, ToNode};
use scry::writer::Document;

#[derive(Config, ToNode)]
struct Preferences {
    /// Multiplier per zoom step.
    #[scry(default = 1.2)]
    zoom_factor: f32,
    /// Budget for retained image data, in MiB.
    #[scry(default = 512)]
    cache_size_mib: usize,
}

let preferences = Preferences { zoom_factor: 1.1, cache_size_mib: 256 };
let document = Document::from_config(&preferences)?;
```

The output contains `1.1` and `256`. Defaults displayed by descriptions never supply values.
To generate a starter file from defaults, first obtain a complete typed value with
`scry::from_defaults::<Preferences>()?`, then pass it to `from_config`. Required values
without defaults must come from the application. Described struct fields use declaration
order. Other map entries retain their supplied order. The derive uses the first paragraph
of each doc comment.

Use `scry::kit::KeyValues<T>` for associative configuration fields. It already supports
reading, writing, and descriptions. Its canonical list-of-pairs output preserves entry
order and duplicate keys. Use `Document::map` when constructing a literal object instead.

Custom types can supply `#[scry(to_node_with(write_value))]` on a field, alongside matching
read and description hooks. For example, a GUI color can serialize as a hex string through
one adapter shared by every writer. See the runnable
[`documented_settings`](../scry/examples/documented_settings.rs) example for color adapters,
nested settings, relative paths, and parse/decode checks:

```sh
cargo run --example documented_settings
```

## Choosing output and layout

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

These writers generate fresh documents. Parsing an existing file into `Node` discards its
comments, formatting, and Rhai expressions. Regenerating that file cannot preserve personal
notes added by hand. An application offering GUI saves should make this ownership clear.
Reading annotated documents or preserving authored source is outside this facility's scope.
