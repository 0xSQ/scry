# Scry

A Rust library for building config-file-driven interfaces and CLI tools on top of them.

*Scry* handles the scaffolding of config-driven applications: parsing files into typed structs, generating format documentation, validating user input with helpful errors, and letting users inspect and override values at runtime. As its primary use case, the library offers utilities for assembling clap-based CLI tools out of these components, with minimal boilerplate.

It also has built-in support for [Rhai](https://rhai.rs) scripts as config files, enabling variables, imports, and computed values. (Which may be what you always wanted, you just didn't know it yet.)

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
- You parse your file into a `scry::Node`
- You convert that into your struct with `as_type<T>()`

And the `scry::Config` derive generates the code needed for the second step automatically.

## But Why?

If you are familiar with [serde](https://serde.rs), you probably recognize this pattern. In `serde`, you parse your JSON into an intermediate tree of `serde_json::Value` objects and then convert that into your struct, although you would typically do it all in one step. So why not just use `serde`? If all you want is to serialize your files into your structs, you probably should. Scry's focus is on applications where you want to do more than just directly translate your data. Where you want to inspect or override your configuration before converting it into your application's types. And where you don't want to do this with language-specific intermediate representations like `serde_json::Value`, or `toml::Value`.

This is the point of Scry's own `scry::Node` tree. It is basically a generic version of your typical config tree representation, but one whose primitives are not tied to the quirks of some particular source language. The primitive types directly map onto those natively used by Rust. All the utility code you write for this intermediate representation will be reusable across all file formats that can be converted into a `scry::Node` tree, present and future ones.

## Building a CLI with Scry

What exactly can you do with this? To further motivate things, let's build a small CLI tool. Say we have a deployment pipeline we want to configure. And it comes in the form of a function that takes a single struct argument with all the relevant parameters:

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

With the following input struct:

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

And a corresponding config file `deploy.json` to read from:

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

Scry includes a `setup` module with a core builder type called `Setup` that lets you wire together a clap command to do just this with minimal boilerplate:

```rust
use scry::cli::setup::{Setup, SetupError};

// ... (structs and function as above) ...

fn main() -> Result<(), SetupError> {
    Setup::standard("deploy").into_bundle(deploy).run()?;
    Ok(())
}
```

`Setup` comes with sensible defaults that can be overridden. By default, it takes the config file path as a positional argument and uses `Node::parse_file` to load it.

(Scry re-exports clap as `scry::clap`, since the builder's extension points traffic in clap types like `Arg` and `Command`. Your crate gets the exact clap version Scry was built against, with no second dependency to keep in sync.)

The `into_bundle` method combines your setup with a target *payload* function and returns a `Bundle` - an object that pairs the generated clap command with its dispatch logic. Calling `run()` parses arguments and dispatches to the target function. Running it shows:

```
$ deploy deploy.json
Deploying to: app.example.com
Environment: dev
Retries: 3
Sending notifications to: https://hooks.slack.com/myhook
```

### Support Options

By default, the setup builder augments your clap command with a set of standard support options for inspecting and overriding config values at runtime. These are enabled by default with the `standard` constructor above. They are shown in the help output:

```
$ deploy --help
Usage: deploy [OPTIONS] [CONFIG]

Arguments:
  [CONFIG]  Path to config file

Options:
  -h, --help  Print help

Config options:
      --desc [<KEY>]           Prints config description and exits (no key = full description)
      --set <KEY> <VALUE>      Sets a config value (can be repeated)
      --remove <KEY>           Removes a config value (can be repeated)
      --get [<KEY>]            Prints config value and exits (no key = whole config)
      --get-flat [<KEY>]       Prints config as flat key=value lines
      --get-as <FORMAT> <KEY>  Prints config in specified format (json, rhai, etc.)
```

To disable these options, use the vanilla `Setup::new` constructor instead of `Setup::standard`.

Let's look at what these options do.

#### `--desc` Option

`--desc` is the `--help` equivalent for your config file. It displays the expected config file format as a fancy schema-like description tree:

```
$ deploy --desc
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

Required fields (`◆`), optional fields (`◇`), enum variants (`›`/`»`), and displayable default values (`→`) are all indicated visually. Comments are pulled from your struct's doc comments automatically. Symbols, spacing, and other formatting details can be customized if desired.

Subsections can be queried by path:

```
$ deploy --desc notify
◇ notify                             ‣ Notification settings.
   ◆ slack_url: string               ‣ Slack webhook URL.
   ◇ on_failure_only: bool → false   ‣ Only notify on failure.
```

#### `--get` Options

These options print the config and exit rather than running the command. Useful for a final check on what you're actually about to execute before it's too late. They come in a few flavors.

`--get` outputs in a simple tree format:

```
$ deploy deploy.json --get
▸ target "app.example.com"
▸ environment "dev"
▾ notify
   ▸ slack_url "https://hooks.slack.com/myhook"
```

`--get-flat` outputs everything as `key = value` lines, giving the full key paths:

```
$ deploy deploy.json --get-flat
target = "app.example.com"
environment = "dev"
notify.slack_url = "https://hooks.slack.com/myhook"
```

`--get-as` outputs in a specific writer format (`json`, `rhai`, or custom registered formats). TOML and YAML writers are optionally available via the `format-toml` and `format-yaml` features.

```
$ deploy deploy.json --get-as json
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

All three support querying specific paths:

```
$ deploy deploy.json --get notify.slack_url
"https://hooks.slack.com/myhook"
```

#### `--set` Option

`--set` allows you to override config values on the fly before execution:

```
$ deploy deploy.json --set dry_run true --set retries 5
Performing dry run...
Deploying to: app.example.com
Environment: dev
Retries: 5
Sending notifications to: https://hooks.slack.com/myhook
```

You can combine `--set` with `--get` to check whether your overrides have worked as intended.

Typos produce helpful errors that reference the input description:

```
$ deploy deploy.json  --set notify.on_failure_olny true
Error: Unknown key 'on_failure_olny' in 'notify'

Available fields:
◆ slack_url: string               ‣ Slack webhook URL.
◇ on_failure_only: bool → false   ‣ Only notify on failure.
```

The same validation happens for config files themselves - unknown keys are caught during parsing, not silently ignored.

### Exposing Config Values as CLI Arguments

Frequently used values can be promoted to proper CLI arguments via the `expose` method. Value-taking
arguments use `option`, while `flag` creates a presence-only argument that applies a predeclared
`--set PATH VALUE` operation:

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

Note that config key names will be kebab-cased automatically (see `--dry-run` above), unless explicitly overridden (see `--env`).

Now users can write:

```
$ deploy deploy.json -n -e production
```

Flags can assign any value supported by `ToNode`, not only boolean `true`. For example,
`e.flag("notify.on_failure_only", false).long("always-notify")` creates `--always-notify` as a
zero-argument shorthand for `--set notify.on_failure_only false`. If the flag is absent, the loaded
config remains unchanged.

When several fixed assignments form one named CLI concept, declare a preset instead:

```rust
e.preset("safe")
    .set("dry_run", true)
    .set("parallelism", 1_usize)
    .help("Runs one operation at a time without applying changes.");
```

`flag(path, value)` is the concise one-assignment form of this same fixed-assignment mechanism. A
preset names the CLI argument independently from its config paths, so it does not inherit help from
any one assignment. Give presets explicit help unless the name is entirely self-explanatory.

Flags, presets, and `--set` can be used together. Each operation is applied in command-line order,
so the later argument wins when several operations target the same path.

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
$ deploy deploy.rhai --get environment
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
- **[Files](docs/files.md)** - Cookbook-style file set selection for Scry configs, with examples
  written in Rhai.

See the [examples](scry/examples) directory for complete working code.

## Name

The Cambridge Dictionary has the following definition of the word *scry*: "to see what will happen in the future, especially by looking into an object such as a mirror or glass ball". In that vein, this here library allows us to look through the crystal ball to receive the data for our magical commandments from the great beyond.

(It's also a catchy 4-letter word likely related to "describe". And it happens to rhyme with "Rhai".)

## Status

Experimental.

## License

Scry is dual-licensed under MIT OR Apache-2.0.
