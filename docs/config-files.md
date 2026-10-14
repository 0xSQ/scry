# Choosing where configuration files live

`Setup` separates finding a configuration file from loading its contents. This lets an
application offer a simple `-C file.rhai` option, discover everyday defaults, or keep most
settings in a shared folder with a few local exceptions.

This guide uses an imaginary document tool, atlas, with `catalog` and `export` commands. Its
catalog settings are the same on several computers. Export settings mostly match, but each
computer writes its output somewhere different.

We'll start by selecting a file explicitly, then add default discovery, a shared directory,
and a small local customization.

## Start with an explicit file

An application using `Setup::standard("catalog")` accepts a config file through `-C` /
`--config-file`:

```console
atlas catalog -C experiment.rhai
atlas catalog -C experiment.rhai -S get
```

The first command loads that file and runs the catalog operation. The second prints its
configuration without running the operation. See the
[README](../README.md#configuration-toolbox) for the inspection and editing tools.

An explicit path takes precedence over any defaults the application discovers. Without a
file, `Setup::standard` reports an error when configuration is needed. Help and `-S desc`
still work. For commands whose settings come from defaults and CLI arguments, `Setup::new`
starts with an empty map instead.

## Enable discovery in the application

`Setup::standard("catalog")` accepts a file through `-C`, but does not discover defaults by
itself. Add `require_command_config` to opt into the conventions in this guide:

```rust
use scry::cli::setup::{require_command_config, Setup, SetupError};
use scry::cli::Bundle;
use scry::Config;

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config)]
struct Catalog {
    /// Categories to include in the catalog.
    categories: Vec<String>,
    /// Includes hidden documents.
    #[scry(default = false)]
    include_hidden: bool,
}

fn setup(command: &str) -> Setup {
    Setup::standard(command).config_source(|source| {
        source.discover(|command| require_command_config("atlas", &[command]).map(Some))
    })
}

fn main() -> Result<(), SetupError> {
    let catalog = setup("catalog").into_bundle(|config: Catalog| {
        println!("Categories: {:?}", config.categories);
        println!("Include hidden documents: {}", config.include_hidden);
    });
    Bundle::group("atlas", "Organizes documents.", vec![catalog]).run()?;
    Ok(())
}
```

`atlas catalog` now looks for `~/.config/atlas/catalog.*`. This convention also uses `.config`
on Windows. The file extension selects the parser. Rhai, JSON, and JSON5 are available by
default. TOML and YAML require their corresponding format features. Two matching files, such as
`catalog.rhai` and `catalog.json`, produce an ambiguity error.

For a nested command, pass a path such as `&["archive", "create"]` to discover
`archive.create.*`. An application can also choose a common file stem for several operations.

To make discovered defaults optional, use `resolve_command_config` rather than
`require_command_config`, followed by `.or_empty()` on the source. This starts from an empty
map when discovery returns `Ok(None)`. Invalid configs and missing explicit files still
produce errors.

## Share a directory of defaults

Keep the shared configs in a synced folder or checked-out repository:

```text
~/projects/atlas-config/
    catalog.rhai
    export.rhai
```

The catalog config is an ordinary map:

```rhai
// ~/projects/atlas-config/catalog.rhai
#{ categories: ["manuals", "reports"], include_hidden: false }
```

Create an app-level file under the local config directory containing the shared directory:

```rhai
// ~/.config/atlas/atlas.rhai
"~/projects/atlas-config"
```

When there is no local `catalog.*`, atlas discovers `catalog.rhai` in that directory. The
app-level file describes locations, rather than command settings. Its paths expand a leading
`~/` or `~\`. Relative paths resolve beside the app-level file.

## Keep local exceptions

Add `~/.config/atlas/export.rhai` when a computer needs different export settings. That file
takes precedence over the shared `export.rhai` and supplies the whole configuration.

For small differences, import the shared settings and modify them. Make the shared file export
its config and return it, so the file also works when loaded directly:

```rhai
// ~/projects/atlas-config/export.rhai
export let config = #{
    output_dir: "exports",
    format: "pdf",
};

config
```

Then use a local wrapper:

```rhai
// ~/.config/atlas/export.rhai
import "../../projects/atlas-config/export.rhai" as defaults;

let config = defaults::config;
config.output_dir = "D:/Documents/AtlasExports";
config
```

Rhai imports resolve relative to the importing file. Absolute paths work too, but imports do
not expand `~`. The wrapper explicitly decides what changes. Assign a field to replace it or
use `+=` to append to an array.

## Share settings with local machine values

When several shared configs need the same local paths, an application can provide an `env`
object instead of requiring a wrapper for each command. Use a local `env.rhai` in place of the
`export.rhai` wrapper from the previous example, and keep the machine's paths there:

```rhai
// ~/.config/atlas/env.rhai
#{ output_dir: "D:/Documents/AtlasExports" }
```

The shared export config can then use that value:

```rhai
// ~/projects/atlas-config/export.rhai
#{ output_dir: env.output_dir, format: "pdf" }
```

The application opts in through `ConfigSource::loader`. Scry's
[`load_app_rhai_env`](../scry/src/cli/setup/rhai_env.rs) helper finds `env.*` using the same
discovery and routing rules as command configs. It builds a `RhaiEnv` containing the loaded
value as the constant `env`:

```rust
use std::path::Path;

use rhai::Engine;
use scry::cli::setup::{load_app_rhai_env, require_command_config, Setup};
use scry::util::PathExt;
use scry::{BoxedError, Node};

// ---------------------------------------------------------------------------------------------- //

fn load_config(path: &Path) -> Result<Node, BoxedError> {
    if !path.ext_str()?.eq_ignore_ascii_case("rhai") {
        return Ok(Node::parse_file(path)?);
    }

    let env = load_app_rhai_env("atlas")?;
    let mut engine = Engine::new();
    engine.set_module_resolver(env.module_resolver());
    Ok(Node::from_rhai_dynamic(env.eval(&engine, path)?)?)
}

fn setup(command: &str) -> Setup {
    Setup::standard(command).config_source(|source| {
        source
            .discover(|command| require_command_config("atlas", &[command]).map(Some))
            .loader(load_config)
    })
}
```

The constant is available in the command script and its imports. This example applies the
environment only to Rhai files and uses the usual parser for other formats.

A local `env.*` replaces the shared environment as a whole. A missing environment file leaves
`env` undefined. An invalid file reports an error. For Rhai, an `env` binding supplies the
value when present, otherwise the final expression does.

The environment file is evaluated separately, before the command's engine is configured. It
can use relative imports, but cannot use the injected `env` or application-specific functions.
For different conventions, use [`RhaiEnv`](../scry/src/rhai.rs) directly to add constants and
evaluate scripts.

## Load a named Rhai config

An application may store several named configs in one file:

```rhai
// ~/.config/atlas/export.rhai
export let print_settings = #{ output_dir: "exports/print", format: "pdf" };
export let web_settings = #{ output_dir: "exports/web", format: "html" };
```

Select a variable with `rhai_var_loader`:

```rust
use scry::cli::setup::{require_command_config, Setup};

// ---------------------------------------------------------------------------------------------- //

let setup = Setup::standard("export").config_source(|source| {
    source
        .discover(|_| require_command_config("atlas", &["export"]).map(Some))
        .rhai_var_loader("print_settings")
});
```

The same discovery and local precedence rules apply. The selected file must be Rhai and define the
requested variable.

## Route individual commands when needed

For configs scattered across directories, the app-level file also supports an optional
`commands` map:

```rhai
// ~/.config/atlas/atlas.rhai
#{
    dir: "~/projects/atlas-config",
    commands: #{
        "export": "~/projects/export-experiments/export.rhai",
    },
}
```

The resolution order is local command file, matching `commands` entry, then a file in `dir`.
Both `dir` and `commands` are optional. A missing explicit target is an error. An entry points
to a concrete config file, not another routing file.

For nested commands, `"archive.create"` matches that operation. A group key such as `archive`
matches every operation below it. The most specific entry wins. Quote Rhai keywords used as
map keys, such as `"export"`.

The app-level file can use any supported format. In Rhai, return the routing value as the final
expression or define a conventional `config` binding. Local same-name files still take
precedence, so most setups only need the directory string.
