# Configuration files

When you use a command often, you probably have a few settings you want to keep. You can save
them in a config file and pass it with `-C`:

```console
atlas catalog -C catalog.rhai
```

Scry can also give those files a default home, so everyday use becomes just `atlas catalog`.
You can start with a file for each command, keep related commands together, or put everything in
one file. The settings stay the same as you rearrange them.

This guide uses an imaginary document tool called `atlas`. Automatic file lookup is something
the application enables. If you're building a CLI, [the last section](#adding-this-to-your-cli)
shows how to turn it on.

## Start with one file

For `atlas catalog`, save a file at `~/.config/atlas/catalog.rhai`:

```rhai
#{ categories: ["manuals", "reports"] }
```

Now run:

```console
atlas catalog
```

Scry finds the file and uses its settings for the command. You can still override settings on
the command line or inspect them with `atlas catalog -S get`. See the
[README's configuration support section](../README.md#configuration-support) for those options.

The file contains the command's settings directly. In Rhai, `#{ ... }` creates a map, and the
last expression is the value the file returns. If you prefer JSON, use `catalog.json` instead:

```json
{ "categories": ["manuals", "reports"] }
```

Rhai, JSON, and JSON5 are available by default. Applications can also enable TOML, YAML, or other
formats. Choose one file for the command. Keeping both `catalog.rhai` and `catalog.json` would
leave Scry with two choices, so it reports an error.

Each application has its own directory under `~/.config/`, including on Windows, where `~`
means your user home directory. The application chooses its directory name, which may differ
from the executable's name.

## Keep related commands together

Suppose `atlas` also has `archive create` and `archive prune` commands. You can give each its
own file, following the command names with directories:

```text
~/.config/atlas/
    catalog.rhai
    archive/
        create.rhai
        prune.rhai
```

`archive/create.rhai` holds the settings for `atlas archive create`:

```rhai
#{ compress: true }
```

`archive/prune.rhai` holds the settings for `atlas archive prune`:

```rhai
#{ keep_days: 30 }
```

As an alternative, put both in `archive.rhai`, using the child command names as keys:

```rhai
#{
    create: #{ compress: true },
    prune: #{ keep_days: 30 }
}
```

Running `atlas archive prune` now picks the `prune` settings from that file. For deeper commands,
the same pattern continues. Each directory or map key follows the next command name.

You can even keep all the settings in an application-wide `atlas.rhai`:

```rhai
#{
    catalog: #{ categories: ["manuals", "reports"] },
    archive: #{
        create: #{ compress: true },
        prune: #{ keep_days: 30 }
    }
}
```

These are different ways to organize the same settings. You can also mix them, keeping `catalog`
in `atlas.rhai` and the archive commands in separate files. A directory does not need a matching
parent file.

When you move a command's settings, remove its old definition. Scry expects each command to get
its settings from one place. Defining them twice is an error. Settings from files are not merged.
Putting settings in a parent file also does not make them defaults for all its children.

Use command names exactly as they appear in the CLI. For example, a `segment-setup` command uses
`segment-setup.rhai` or a `"segment-setup"` key. Rhai keys containing hyphens or language keywords
need quotes. At levels that list commands, Scry checks the names and catches typos. Within a
command's settings, the application defines the available fields.

## When a command also has subcommands

Sometimes a command can run on its own as well as having children. For example, `atlas archive`
might show a summary, while `atlas archive create` creates an archive. Both need settings.

In this case, put the parent's own settings under `"$self"` in `archive.rhai`:

```rhai
#{
    "$self": #{ verbose: false },
    create: #{ compress: true },
    prune: #{ keep_days: 30 }
}
```

`atlas archive` gets `{ verbose: false }`. Its children get their respective values. `$self`
keeps the parent's settings separate from the child command names. You only need it when the
command has children and also runs on its own.

The same rule applies to the top-level command. If `atlas` itself runs an operation, its settings
go under `$self` in `atlas.rhai`, beside `catalog` and `archive`. A small tool with no subcommands
can put its settings directly in its application-named file.

## Choose a file for one run

Use `-C` or `--config` to try another config without changing your usual files:

```console
atlas catalog -C experiment.rhai
```

This uses the whole value from `experiment.rhai`. Its filename does not have to match the command.
Give a path relative to your current directory, or an absolute path. The usual command files are
not consulted for this invocation.

If the file holds settings for several commands, add `::` followed by the part you want:

```console
atlas archive create -C archive.rhai::create
atlas archive prune -C atlas.rhai::archive.prune
```

The selector starts at the file's returned value. Dots step into nested maps, and brackets can
select array elements or quoted keys. For the parent's own settings from the previous example:

```console
atlas archive -C 'archive.rhai::["$self"]'
```

The single quotes prevent shells such as PowerShell from expanding `$self`. Ordinary Windows
paths work too, for example `-C 'C:\configs\atlas.rhai::archive.prune'`. For unusual filenames
containing `::`, see the escaping rules on
[`ConfigFile::parse`](../scry/src/cli/setup/config_file.rs).

After Scry selects the settings, inspection and edits work from there. For example,
`atlas archive prune -C atlas.rhai::archive.prune -S get keep_days` reads `keep_days` directly.
You do not have to repeat `archive.prune` in the query.

## Share values between Rhai configs

Rhai lets you calculate settings or reuse values. Within a single file, an ordinary variable
is often enough:

```rhai
let retention = 30;
#{
    create: #{ compress: true },
    prune: #{ keep_days: retention }
}
```

You can compute the whole returned map too:

```rhai
fn archive_settings(retention) {
    #{ create: #{ compress: true }, prune: #{ keep_days: retention } }
}

archive_settings(30)
```

Scry evaluates the files it checks during discovery, including parent files that do not supply
this command's settings. Their imports and other expressions run too, so an error in a parent can
stop the command. Scry reuses the chosen file's result and loads the shared environment once.

For values shared across separate files, you can use Rhai imports or an optional `env.*` file in
the application's config directory. For example, `env.rhai` could contain:

```rhai
#{ archive_retention_days: 30 }
```

Scry makes that returned value available as `env` in the other Rhai configs and their imports.
Then `archive/prune.rhai` can use it:

```rhai
#{ keep_days: env.archive_retention_days }
```

The environment file can use another enabled format too. An `env.json` containing
`{ "archive_retention_days": 30 }` supplies the same value to Rhai scripts. A JSON command config
remains ordinary JSON and cannot refer to Rhai variables.

The environment is optional. If you use it, make sure the file returns the value you want to
share. For example, a Rhai file that defines `let settings = ...;` should end with `settings`.
The application can choose a different environment filename or variable name, as shown below.

## Keep your configs somewhere else

You may want your configs in a version-controlled directory or another shared location. A
filesystem link from `~/.config/atlas/` to that directory works. You can also link individual
files or command directories.

For a simple alternative to linking the whole directory, put a `root.rhai` in
`~/.config/atlas/`:

```rhai
#{ directory: "~/configs/atlas" }
```

Scry then looks in `~/configs/atlas/` for everything, including the optional environment file:

```text
~/configs/atlas/
    env.rhai
    catalog.rhai
    archive.rhai
```

`root.rhai` must be the only entry in the original `~/.config/atlas/` directory. Move the other
files to the destination. The pointer replaces the directory as a whole, so there is only one
place to look for settings. The destination must already exist and cannot contain another root
pointer.

The pointer can use any enabled format. For example, `root.json` can contain
`{ "directory": "~/configs/atlas" }`. A path starting with `~/` refers to your home directory.
A relative path is resolved beside the pointer file. If that file is itself a link, the path is
relative to its physical location.

## Adding this to your CLI

The examples above use Scry's standard file conventions. To enable them, add
`.application("atlas")` to each command's config source:

```rust
use scry::cli::setup::{Setup, SetupError};
use scry::cli::Bundle;
use scry::Config;

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Config)]
struct Catalog {
    /// Categories to include in the catalog.
    categories: Vec<String>,
}

fn main() -> Result<(), SetupError> {
    let catalog = Setup::standard("catalog")
        .config_source(|source| source.application("atlas"))
        .into_bundle(|config: Catalog| println!("{config:?}"));
    Bundle::group("atlas", "Organizes documents.", vec![catalog]).run()?;
    Ok(())
}
```

`Setup::standard` provides `-C`. Adding `application` lets users omit it and use the saved files
described in this guide. Build nested commands with `Bundle::group` and `with_subcommands`, and
Scry follows their names automatically. If a missing config should use defaults and command-line
input instead, add `.or_empty()` after `.application("atlas")`.

For shared options, create an `ApplicationConfig` and pass a clone to each command. For example,
you can choose `machine.*` as the environment filename and `host` as the Rhai variable name:

```rust
use scry::cli::setup::{ApplicationConfig, Setup};

// ---------------------------------------------------------------------------------------------- //

let application = ApplicationConfig::new("atlas")
    .environment_file("machine")
    .environment_binding("host");

let catalog = Setup::standard("catalog")
    .config_source(|source| source.application(application.clone()));
```

Command scripts would then use `host.archive_retention_days`. The environment file's returned
value stays the same. Renaming the file also frees `env.rhai` for a command called `env`.
With the default names, that command's settings can instead live under an `env` key in
`atlas.rhai`. The names `root.*`, the environment filename, and the application-wide filename
have special roles at the top of the config directory.

The [complete example](../scry/examples/command_configs.rs) uses these custom environment names
and adds the archive commands, a custom Rhai function, and reporting of the selected config file.
For API details and the full discovery rules, see the doc comments on
[`ApplicationConfig`](../scry/src/cli/setup/command_config.rs) and
[`ConfigSource`](../scry/src/cli/setup/config_source.rs).
