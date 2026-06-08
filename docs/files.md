# Files

`scry::kit::Files` describes a set of files in two steps:

- `from` says where to look.
- `where` says which discovered files to keep.

The type is meant to be pleasant to write in any Scry-supported config format, including JSON,
TOML, YAML, and Rhai. The examples below use Rhai because it is compact and readable, but the same
shapes apply across formats. Start with the shortest form that does what you need, then expand into
a structured map when the selection needs more rules.

## Quick Start

Select every file under one directory:

```rhai
files: "assets"
```

Select every file under directories whose names start with `202`:

```rhai
files: "archive/202*"
```

Select image files under those matching directories:

```rhai
files: #{
    from: "archive/202*",
    where: #{ ext: ["png", "jpg", "jpeg", "webp", "gif"] },
}
```

Exclude thumbnails and cache files:

```rhai
files: #{
    from: "archive/202*",
    where: #{
        ext: ["png", "jpg", "jpeg", "webp", "gif"],
        path: #{ exclude: ["**/thumbs/**", "**/.cache/**"] },
    },
}
```

## Mental Model

`Files` is a list of sources. Each source has:

- `from.root`: files or directories to collect from.
- `from.prune`: directories or paths to skip during collection.
- `where.path`: filters on paths relative to each source root, or absolute paths.
- `where.name`: filters on the final file name, including extension.
- `where.stem`: filters on the file name without extension.
- `where.ext`: filters on the extension.

Results from all sources are merged, deduplicated, and kept in first-seen order.

## Syntax Forms

### One Source String

A string is the simplest source. If it names a directory, Scry walks it recursively.

```rhai
files: "src"
```

### Several Source Strings

An array is a list of sources.

```rhai
files: ["src", "tests", "examples"]
```

### One Structured Source

Use a map when one source needs filters.

```rhai
files: #{
    from: "src",
    where: #{ ext: "rs" },
}
```

### Several Structured Sources

Use `sources` when different roots need different filters.

```rhai
files: #{
    sources: [
        #{ from: "src", where: #{ ext: "rs" } },
        #{ from: "docs", where: #{ ext: "md" } },
    ],
}
```

### Implicit Root

If a source has no `from`, the caller-provided base directory is used as the root.

```rhai
files: #{
    where: #{ ext: "rs" },
}
```

This only works when the Rust caller passes a base directory to `locate`.

## Patterns

There are two public pattern syntaxes:

- `exact`: literal matching.
- `wildcard`: `*`, `?`, and `**` matching.

Plain strings auto-detect syntax:

```rhai
files: "src"       // exact
files: "src/*.rs"  // wildcard
```

Explicit path objects also auto-detect when `syntax` is omitted:

```rhai
files: #{ from: #{ path: "archive/202*" } }
```

Force exact matching when you really want a literal `*` or `?`:

```rhai
files: #{ from: #{ path: "literal-star-*", syntax: "exact" } }
```

`[` and `]` are literal in wildcard syntax. There is no separate public glob syntax.

## Include and Exclude Rules

Every `where` filter accepts the same shapes.

One include pattern:

```rhai
where: #{ ext: "rs" }
```

Several include patterns:

```rhai
where: #{ ext: ["rs", "toml", "md"] }
```

Full include/exclude rule:

```rhai
where: #{
    name: #{
        include: "*.rs",
        exclude: "*.generated.rs",
    },
}
```

Exclude wins over include. If `include` is omitted or empty, everything is included unless an
exclude pattern rejects it.

## Cookbook

### One Directory Recursively

```rhai
files: "project/src"
```

### Several Directories

```rhai
files: ["project/src", "project/tests", "project/examples"]
```

### Directories by Prefix

```rhai
files: "archive/202*"
```

This matches roots like `archive/2020`, `archive/2021`, and `archive/2024`, then walks each matched
directory.

### Image Files Under Matching Roots

```rhai
files: #{
    from: "archive/202*",
    where: #{ ext: ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif"] },
}
```

### Exclude Thumbnails and Cache Directories

```rhai
files: #{
    from: "images",
    where: #{
        ext: ["png", "jpg", "webp"],
        path: #{ exclude: ["**/thumbs/**", "**/.cache/**"] },
    },
}
```

### Source Files but Not Generated Files

```rhai
files: #{
    from: "src",
    where: #{
        ext: "rs",
        name: #{ exclude: "*.generated.rs" },
    },
}
```

### Only Files Below a Subdirectory

```rhai
files: #{
    from: ["project-a", "project-b"],
    where: #{ path: "src/**/*.rs" },
}
```

The `path` pattern is relative to each source root, so this finds `src/**/*.rs` inside both
`project-a` and `project-b`.

### Exclude Files by Relative Path

```rhai
files: #{
    from: "project",
    where: #{ path: #{ exclude: ["target/**", "dist/**"] } },
}
```

### Match by Full File Name

```rhai
files: #{
    from: "docs",
    where: #{ name: "README.md" },
}
```

### Match by File Stem

```rhai
files: #{
    from: "docs",
    where: #{ stem: ["README", "CHANGELOG"] },
}
```

### Match Extension With or Without Dot

These are equivalent:

```rhai
files: #{ from: "src", where: #{ ext: "rs" } }
files: #{ from: "src", where: #{ ext: ".rs" } }
```

### Case-Sensitive Matching

`where` matching is case-insensitive by default. Set `case` to `sensitive` when capitalization
matters.

```rhai
files: #{
    from: "docs",
    where: #{
        case: "sensitive",
        name: "README.md",
    },
}
```

### Optional Root

Exact roots must exist by default. Wildcard roots may match nothing by default.

```rhai
files: #{
    from: #{ path: "optional-reports", must_exist: false },
}
```

### Non-Recursive Root

Set `recursive` to `false` to collect only direct children of a directory root.

```rhai
files: #{
    from: #{ path: "downloads", recursive: false },
}
```

### Prune Large Directories

Use `from.prune` when you want traversal to skip a directory entirely.

```rhai
files: #{
    from: #{
        root: "project",
        prune: ["project/target", "project/node_modules"],
    },
    where: #{ ext: ["rs", "toml", "md"] },
}
```

Use `where.path.exclude` for ordinary result filtering. Use `from.prune` when skipping the walk
itself matters.

### Different Rules for Different Roots

```rhai
files: #{
    sources: [
        #{
            from: "src",
            where: #{ ext: "rs" },
        },
        #{
            from: "docs",
            where: #{ ext: "md" },
        },
        #{
            from: "assets",
            where: #{ ext: ["png", "jpg", "webp"] },
        },
    ],
}
```

## Reference

### `Files`

```rhai
files: "path/or/pattern"
files: ["path/a", "path/b"]
files: #{ from: "...", where: #{ ... } }
files: #{ sources: [ ... ] }
```

### `from`

```rhai
from: "path/or/pattern"

from: #{
    root: "path/or/pattern",
    prune: "path/or/pattern",
}
```

Fields:

- `root`: one or more roots to collect from.
- `prune`: one or more paths or patterns to exclude. Exact directory prunes skip traversal.

### Root Path Objects

```rhai
from: #{
    root: #{
        path: "archive/202*",
        syntax: "wildcard",
        must_exist: false,
        recursive: true,
    },
}
```

Fields:

- `path`: the path or wildcard pattern.
- `syntax`: optional, either `exact` or `wildcard`. Omit it for auto-detection.
- `must_exist`: optional. Defaults to `true` for exact patterns and `false` for wildcard patterns.
- `recursive`: optional. Defaults to `true`.

### `where`

```rhai
where: #{
    case: "insensitive",
    path: "src/**/*.rs",
    name: "*.rs",
    stem: "main",
    ext: ["rs", "toml"],
}
```

Fields:

- `case`: `insensitive` or `sensitive`. Defaults to `insensitive`.
- `path`: matches absolute paths or paths relative to each source root.
- `name`: matches the final path component with extension.
- `stem`: matches the final path component without extension.
- `ext`: matches the extension. A leading dot is ignored.

### Symlinks

Symlinks to files are included. Symlinked directories are not traversed. A root that directly names
a symlinked directory is an error.

### Errors

Relative roots need a base directory from the Rust caller. Exact roots must exist unless
`must_exist: false` is set. Wildcard roots may match nothing unless `must_exist: true` is set.
