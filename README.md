# Scry 🔮

A Rust library for building config-file-driven interfaces and CLI tools on top of them.

*Scry* helps you build Rust applications around configuration files. It loads settings into typed structs, generates documentation from your types, and reports invalid input with helpful errors. Its clap-based CLI utilities let users inspect and override those settings from the command line.

It also supports [Rhai](https://rhai.rs) scripts as config files, enabling variables, imports, and computed values. (Which may be what you always wanted, you just didn't know it yet.)

## Example

Here's how you use it to parse a file into a struct:

```rust
use scry::{Config, Node};

#[derive(Debug, Config)]
struct Deploy {
    target: String,
    #[scry(default = 3)]
    retries: u32,
}

// `deploy.json` contains { "target": "app.example.com" }
let node = Node::parse_file("deploy.json")?;
let config: Deploy = node.as_type()?;

// Prints: "Deploying to app.example.com with 3 retries"
println!("Deploying to {} with {} retries", config.target, config.retries);
```
There are two steps:

1. Parse the file into a `Node`.
2. Convert it into your struct with `as_type::<T>()`.

The `Config` derive generates the conversion code for the second step.

## But Why?

If you're familiar with [serde](https://serde.rs), you probably recognize this pattern: parse a file into a struct, often in a single step. So why not just use `serde`? For straightforward serialization and deserialization, you probably should. Scry focuses on applications where you also want to inspect or override configuration before converting it into your application's types, without tying that logic to a format-specific tree such as `serde_json::Value` or `toml::Value`.

This is the point of Scry's `Node` tree. Its primitives map directly to Rust's native types, giving configuration code a common representation across input languages. Utilities written against it can work with any file format that can be converted into a `Node`.

## Building a CLI with Scry

To see how these pieces fit together, let's build a small CLI for a deployment pipeline. We'll start with a function that takes a struct containing its settings:

```rust
// Our deployment function. Just prints things for now.
fn deploy(config: Deploy) {
    if config.dry_run {
        println!("Performing dry run...");
    }
    println!("Deploying to: {}", config.target);
    println!("Environment: {}", config.environment);
    println!("Retries: {}", config.retries);
    if let Some(timeout) = config.timeout_secs {
        println!("Timeout: {}s", timeout);
    }
    if let Some(notify) = config.notify {
        println!("Sending notifications to: {}", notify.slack_url);
        if notify.on_failure_only {
            println!("   (only on failure)");
        }
    }
}
```

Here's the input type, along with its nested settings:

```rust
use scry::Config;

#[derive(Debug, Config)]
struct Deploy {
    /// Target server hostname.
    target: String,
    /// Deployment environment (e.g. "dev", "staging", "prod").
    environment: String,
    /// Number of retry attempts on failure.
    #[scry(default = 3)]
    retries: u32,
    /// Request timeout in seconds.
    timeout_secs: Option<u64>,
    /// Notification settings.
    notify: Option<Notify>,
    /// Where to send log output.
    #[scry(from_defaults)]
    log_output: LogOutput,
    /// Run without making changes.
    #[scry(default = false)]
    dry_run: bool,
}

#[derive(Debug, Config)]
enum LogOutput {
    /// Log to standard output.
    #[scry(default)]
    Stdout,
    /// Log to standard error.
    Stderr,
}

#[derive(Debug, Config)]
struct Notify {
    /// Slack URL to send notifications to.
    slack_url: String,
    /// Only notify on failure.
    #[scry(default = false)]
    on_failure_only: bool,
}
```

And a corresponding `deploy.json`:

```json
{
    "target": "app.example.com",
    "environment": "dev",
    "notify": {
        "slack_url": "https://hooks.slack.com/myhook"
    }
}
```

Let's set up a CLI that loads this file and calls the `deploy` function with it.

### The `Setup` CLI Builder

The `Setup` builder in `scry::cli::setup` wires this function into a clap command:

```rust
use scry::cli::setup::{Setup, SetupError};

// ... (structs and function as above) ...

fn main() -> Result<(), SetupError> {
    Setup::standard("deploy").into_bundle(deploy).run()?;
    Ok(())
}
```

The `into_bundle` method pairs the generated clap command with your *payload* function in a `Bundle`. Calling `run()` parses the arguments and dispatches to that function:

```
$ deploy -C deploy.json
Deploying to: app.example.com
Environment: dev
Retries: 3
Sending notifications to: https://hooks.slack.com/myhook
```

Applications can also opt into automatically loading configuration files from default locations. See the [config file guide](docs/config-files.md) for details.

### Configuration support

Every `Setup` command provides configuration support through `--scry` or its short alias `-S`. The ordinary help lists config selection among the command options and the support switch in a closing `Configuration support` section:

```
$ deploy --help
Usage: deploy [OPTIONS]

Options:
  -C, --config <FILE[::KEY]>  Loads a config file or a selected part of it.
  -h, --help                  Print help

Configuration support:
  -S, --scry [<ARGS>...]  Inspects and adjusts this command's configuration.
```

Put application arguments before `-S`. The support commands have their own help and flags, independent of application flags before `-S`:

```
$ deploy -S
Inspects and adjusts this command's configuration.

Usage: deploy --scry [OPTIONS] [COMMAND]

Commands:
  desc      Describes the configuration type without loading or executing.
  get       Displays the raw edited configuration without decoding or executing.
  check     Decodes the edited configuration without running the command payload.
  template  Generates a documented configuration scaffold without loading or executing.

Edits:
      --set <KEY> <VALUE>  Sets a config value to a literal string before execution, get, or check (repeatable).
      --remove <KEY>       Removes a config value before execution, get, or check (repeatable).
```

Use `-S COMMAND --help` for command-specific help.

#### `desc`

`-S desc` is the `--help` equivalent for your config file. It shows a human-readable overview of config fields, value hints, and defaults:

```
$ deploy -S desc
◆ target: string                     ‣ Target server hostname.
◆ environment: string                ‣ Deployment environment (e.g. "dev", "staging", "prod").
◇ retries: u32 → 3                   ‣ Number of retry attempts on failure.
◇ timeout_secs: u64                  ‣ Request timeout in seconds.
◇ notify                             ‣ Notification settings.
┊  ◆ slack_url: string               ‣ Slack URL to send notifications to.
┊  ◇ on_failure_only: bool → false   ‣ Only notify on failure.
◇ log_output                         ‣ Where to send log output.
┊  » stdout                          ‣ Log to standard output.
┊  › stderr                          ‣ Log to standard error.
◇ dry_run: bool → false              ‣ Run without making changes.
```

The display marks required fields (`◆`), optional fields (`◇`), enum variants (`›`/`»`), and displayable defaults (`→`). Descriptions come from your type's doc comments. Symbols, spacing, and other formatting details are customizable.

Subsections can be queried by path:

```
$ deploy -S desc notify
◇ notify                             ‣ Notification settings.
   ◆ slack_url: string               ‣ Slack URL to send notifications to.
   ◇ on_failure_only: bool → false   ‣ Only notify on failure.
```

#### `get`

`get` prints the raw input after applying exposed arguments and input edits, then exits. It does not decode the input into the application's type or materialize typed defaults. This helps inspect what the command will receive, including input that is incomplete or invalid for execution.

The default output is a simple tree format:

```
$ deploy -C deploy.json -S get
▸ target "app.example.com"
▸ environment "dev"
▾ notify
   ▸ slack_url "https://hooks.slack.com/myhook"
```

`get --as flat` prints one `key value` line per leaf, using full key paths:

```
$ deploy -C deploy.json -S get --as flat
target "app.example.com"
environment "dev"
notify.slack_url "https://hooks.slack.com/myhook"
```

`get --as FORMAT` also supports writer formats such as `json` or `rhai`. TOML and YAML writers are optionally available via the `format-toml` and `format-yaml` features.

```
$ deploy -C deploy.json -S get --as json
{
  "target": "app.example.com",
  "environment": "dev",
  "notify": {
    "slack_url": "https://hooks.slack.com/myhook"
  }
}
```

Format support is registry-based, so built-ins and custom formats use the same parser/writer
registration path.

Every output format supports querying specific paths:

```
$ deploy -C deploy.json -S get notify.slack_url
"https://hooks.slack.com/myhook"
```

#### `check`

`-S check` checks whether the edited configuration decodes successfully, without running the command payload.

#### `template`

`-S template` gives users a starting point for writing their config file, with comments drawn from
the same type documentation as `desc`.

For example, `deploy -S template --as json5 --mode ref` produces this excerpt for `Deploy`:

```json5
{
    // Target server hostname. Required.
    // "target": <string>,

    // Deployment environment (e.g. "dev", "staging", "prod"). Required.
    // "environment": <string>,

    // Number of retry attempts on failure. Optional.
    // "retries": 3, // type: u32
}
```

Fill in and uncomment the required placeholders to make it usable. See [Writing Config Files](docs/writing-configs.md) for the full workflow.

#### Editing Input

`--set` edits a config value before execution. Values are literal strings, so `true`, `5`, and `null` are not parsed as config expressions. Typed conversion interprets those strings according to the target field:

```
$ deploy -C deploy.json -S --set dry_run true --set retries 5
Performing dry run...
Deploying to: app.example.com
Environment: dev
Retries: 5
Sending notifications to: https://hooks.slack.com/myhook
```

Put edits before `get` to inspect their result or before `check` to test decoding. An edit-only suffix executes the application with the edited input:

```
$ deploy -C deploy.json -S --set retries 5 get retries
"5"
```

`--remove KEY` removes a value from the input.

Typos produce helpful errors that reference the input description:

```
$ deploy -C deploy.json -S --set notify.on_failure_olny true
Error: Unknown key 'on_failure_olny' in 'notify'

Available fields:
◆ slack_url: string               ‣ Slack URL to send notifications to.
◇ on_failure_only: bool → false   ‣ Only notify on failure.
```

The same validation happens for config files themselves - unknown keys are caught during typed conversion, not silently ignored.

### Exposing Config Values as CLI Arguments

Frequently used values can be promoted to proper CLI arguments via the `expose` method. Value-taking
arguments use `option`, while `flag` applies a predeclared assignment:

```rust
use scry::cli::setup::{Setup, SetupError};

fn main() -> Result<(), SetupError> {
    Setup::standard("deploy")
        .expose(|e| {
            e.option("environment").short('e').long("env");
            e.flag("dry_run", true).short('n');
        })
        .into_bundle(deploy)
        .run()?;
    Ok(())
}
```

These appear as first-class options, with the help text again pulled from doc comments:

```
Options:
  -e, --env <VALUE>  Deployment environment (e.g. "dev", "staging", "prod").
  -n, --dry-run      Run without making changes.
  -h, --help         Print help
```

Exposed argument names are kebab-cased automatically (see `--dry-run` above), unless explicitly overridden (see `--env`).

Now users can write:

```
$ deploy -C deploy.json -n -e production
```

Flags can assign any value, not only boolean `true`. For example,
`e.flag("notify.on_failure_only", false).long("always-notify")` creates `--always-notify` as a
zero-argument shorthand for assigning `false` to `notify.on_failure_only`. If the flag is absent, the loaded
config remains unchanged.

When several fixed assignments form one named CLI concept, declare a preset instead:

```rust
e.preset("safe")
    .set("dry_run", true)
    .set("retries", 1_u32)
    .help("Retries once without applying changes.");
```

Flags and presets apply in application argument order. Input edits apply afterward, in suffix
order. The later operation wins when several operations target the same path.

## Rhai

Static config files are great. Declarative, easy to read, just the data, no convoluted logic from your overly clever coworker. But as your config file grows, replicated values and boilerplate tend to creep in and you find yourself copying the same value into multiple places every time you tinker with something. No big deal, you think, but then one day you forget to copy that one parameter to that one other place you should have and your hours-long ML training run is strangely corrupted and it takes you forever to find out what has gone wrong. (Not that such a thing would ever happen to this author.) And that's the day you think: maybe DRY for config files isn't entirely out of place either. And adding one or two variables you can reuse here and there isn't the end of the world.

And this is why Scry supports [Rhai](https://rhai.rs) scripts as an alternative.

At its most basic, a Rhai script returning a Rhai object looks like your average "better JSON":

```rhai
// deploy.rhai
#{
    target: "app.example.com",
    environment: "prod",
    retries: 5,
    timeout_secs: 120,
    notify: #{
        slack_url: "https://hooks.slack.com/myhook",
        on_failure_only: true,
    }
}
```

You have your comments, trailing commas, and unquoted keys. About the only quirk is the use of `#` in front of object curlies. Starting from this, you can add bits of simple logic with fairly obvious syntax:

```rhai
// deploy.rhai
let base_domain = "example.com";
let env = "prod"; // "dev", "staging", or "prod"
#{
    target: `app.${base_domain}`,
    environment: env,
    retries: if env == "prod" { 5 } else { 3 },
    timeout_secs: if env == "prod" { 120 } else { 30 },
    notify: #{
        slack_url: "https://hooks.slack.com/myhook",
        on_failure_only: env == "prod",
    }
}
```

You can use it just like the JSON file before:

```
$ deploy -C deploy.rhai -S get environment
"prod"
```

Among many other useful features, [Rhai](https://rhai.rs) scripts can import other Rhai files, enabling you to factor out common settings:

```rhai
// deploy.rhai
import "defaults.rhai" as defaults;
import "secrets.rhai" as secrets;

defaults::base + #{
    target: "app.example.com",
    notify: #{
        slack_url: secrets::slack_webhook
    }
}
```

## Further

- **[Core Concepts](docs/concepts.md)** - Explains the basics: how the `Node` tree works, how to implement `FromNode` manually, etc.
- **[Configuration Files](docs/config-files.md)** - Starts with one command's settings, then shows
  how to group commands in a file, share values, and keep configs in another directory.
- **[Writing Config Files](docs/writing-configs.md)** - Generates documented Rhai or JSON5 files from values and configuration metadata.
- [`kit`](scry/src/kit.rs) provides reusable input shapes for designing command interfaces. Use these building blocks in your configuration types to give users convenient ways to express common inputs:
  - [`KeyValues<V>`](scry/src/kit/key_values.rs) accepts a map or a list of key-value pairs with unique string keys. The pair form preserves explicit entry order.
  - [`OneOrMany<T>`](scry/src/kit/one_or_many.rs) accepts one value or an array, keeping common single-item inputs brief.
  - [`Files`](scry/docs/kit/files.md) selects files by path or pattern, with optional filtering and sorting.
  - [`SeqExpr`](scry/docs/kit/sequence-expressions.md) expresses numeric sequences and index selections compactly, such as `0..20:2` for every other item among the first twenty.

See the [examples](scry/examples) directory for complete working code.

## Name

The Cambridge Dictionary has the following definition of the word *scry*: "to see what will happen in the future, especially by looking into an object such as a mirror or glass ball". In that vein, this here library allows us to look through the crystal ball to receive the data for our magical commandments from the great beyond.

(It's also a catchy 4-letter word likely related to "describe". And it happens to rhyme with "Rhai".)

## Status

Experimental for-fun personal project. But quite usable for that, methinks!

## License

Scry is dual-licensed under MIT OR Apache-2.0.
