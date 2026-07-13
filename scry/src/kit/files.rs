//! Pattern-based file selection.
//!
//! The [`Files`] utility describes a set of files via a two-phase model:
//!
//! 1. **`from`** - Discovery rules: which roots to walk and which subtrees to prune.
//! 2. **`where`** - Filtering rules: which discovered files to keep.
//!
//! See `docs/files.md` for the user-facing config cookbook.
//!
//! # Symlink behavior
//!
//! Symlinks to files are included. A root path that resolves to a symlinked directory
//! produces a [`FilesError::SymlinkDirNotTraversable`] error. Symlinked directories
//! encountered during traversal are silently skipped. Broken symlinks encountered during
//! traversal are skipped; an exact root pointing to a broken symlink behaves like a
//! missing path (`must_exist` applies).

use crate::desc::{self, Desc};
use crate::kit::OneOrMany;
use crate::node::{Node, NodeError};
use crate::traits::{Describe, FromNode};
use crate::util::{PathError, PathExt};
use crate::ToNode;
use globset::{GlobBuilder, GlobMatcher, GlobSet, GlobSetBuilder};
use indexmap::IndexSet;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------------- //

/// Pattern-based file selection specification.
///
/// Holds one or more [`SourceSpec`]s. Each source defines discovery rules (`from`) and
/// filtering rules (`where`). Results are merged across sources with deduplication.
#[derive(Debug, Clone, Default, ToNode)]
pub struct Files {
    /// The sources to collect files from.
    pub sources: Vec<SourceSpec>,
}

/// Specifies a collection of files via discovery (`from`) and filtering (`where`) rules.
///
/// Can be specified as a string (shorthand for a single `from.root` entry) or a map with
/// optional `from` and optional `where` blocks. If `from` is omitted, the caller-provided
/// `base_dir` is used as the implicit root.
#[derive(Debug, Clone, Default, ToNode)]
pub struct SourceSpec {
    /// Discovery rules: where to walk and what to skip.
    pub from: Option<FromSpec>,
    /// Filtering rules: which discovered files to keep.
    #[scry(rename = "where")]
    pub where_: Option<WhereSpec>,
}

/// Discovery rules for a source.
///
/// Controls where candidate files come from.
///
/// Can be specified as a string (shorthand for a single root) or a map with `root` and
/// optional `prune`.
#[derive(Debug, Clone, Default, ToNode)]
pub struct FromSpec {
    /// Starting locations to walk. Accepts paths and/or patterns.
    pub root: OneOrMany<PathPatternSpec>,
    /// Paths or patterns to exclude. Exact directory paths skip traversal entirely;
    /// wildcard patterns filter results post-collection.
    pub prune: OneOrMany<PathPatternSpec>,
}

/// A path pattern for use in `from.root` and `from.prune`.
///
/// Can be a string shorthand (auto-detected) or an explicit map with `path`, optional
/// `syntax`, and optional `must_exist` and `recursive` fields.
#[derive(Debug, Clone, PartialEq, Eq, ToNode)]
pub struct PathPatternSpec {
    /// The path text.
    pub path: String,
    /// How the pattern is interpreted.
    pub syntax: PatternSyntax,
    /// Whether at least one path must exist or match. Defaults to `true` for exact patterns,
    /// `false` for wildcard patterns. Enforced for all `root` patterns and exact
    /// `prune` paths; not enforced for wildcard `prune`.
    pub must_exist: bool,
    /// Controls directory recursion when the pattern resolves to a directory. For `root`
    /// patterns (exact or wildcard-matched), controls walk depth. For exact directory `prune`,
    /// `false` restricts to direct children only. Not used for wildcard `prune`
    /// (those are path filters). Defaults to `true`.
    pub recursive: bool,
}

impl PathPatternSpec {
    /// Creates a path pattern by auto-detecting syntax from wildcards in the string.
    pub fn auto_detect(s: String) -> Self {
        auto_detect_path_pattern(s)
    }
}

/// Controls how a pattern string is interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ToNode)]
#[scry(rename_all = "snake_case")]
pub enum PatternSyntax {
    /// Literal string/path matching.
    #[default]
    Exact,
    /// Shell-like matching (`*`, `?`, `**`). `[` and `]` are treated as literal characters.
    Wildcard,
}

/// Filtering rules for discovered files.
///
/// Each filter (`path`, `name`, `stem`, `ext`) uses the same include/exclude model.
/// Multiple attributes combine with AND semantics. Case sensitivity is controlled by `case`.
#[derive(Debug, Clone, Default, ToNode)]
pub struct WhereSpec {
    /// Case sensitivity mode for all attribute matching. Defaults to insensitive.
    pub case: CaseMode,
    /// Filter on the root-relative or absolute file path.
    pub path: Option<AttrRuleSpec>,
    /// Filter on the full file name (basename with extension).
    pub name: Option<AttrRuleSpec>,
    /// Filter on the file stem (basename without extension).
    pub stem: Option<AttrRuleSpec>,
    /// Filter on the file extension.
    pub ext: Option<AttrRuleSpec>,
}

/// Controls case sensitivity for attribute matching in `where` blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ToNode)]
#[scry(rename_all = "snake_case")]
pub enum CaseMode {
    /// Case-insensitive matching (the default).
    #[default]
    Insensitive,
    /// Case-sensitive matching.
    Sensitive,
}

/// An include/exclude rule for a file attribute.
///
/// Supports three config forms:
/// - String shorthand: `"rs"` (equivalent to `#{ include: "rs" }`).
/// - Array shorthand: `["rs", "md"]` (equivalent to `#{ include: ["rs", "md"] }`).
/// - Full map: `#{ include: ["rs"], exclude: ["bak"] }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, ToNode)]
pub struct AttrRuleSpec {
    /// Patterns a file attribute must match. If empty, all values are allowed.
    pub include: OneOrMany<TextPatternSpec>,
    /// Patterns that reject a file. Exclude wins over include.
    pub exclude: OneOrMany<TextPatternSpec>,
}

/// A text pattern for matching file attributes and root-relative paths.
///
/// Can be a string shorthand (auto-detected) or an explicit map with `pattern` and optional
/// `syntax`.
#[derive(Debug, Clone, PartialEq, Eq, ToNode)]
pub struct TextPatternSpec {
    /// The pattern text.
    pub pattern: String,
    /// How the pattern is interpreted.
    pub syntax: PatternSyntax,
}

impl TextPatternSpec {
    /// Creates a text pattern by auto-detecting syntax from wildcards in the string.
    pub fn auto_detect(s: String) -> Self {
        auto_detect_text_pattern(s)
    }

    /// Creates an exact text pattern.
    pub fn exact(s: String) -> Self {
        Self {
            pattern: s,
            syntax: PatternSyntax::Exact,
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Error Type

/// Errors that can occur during file resolution.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum FilesError {
    /// Invalid wildcard pattern in a root spec.
    #[error("invalid root wildcard pattern '{pattern}'")]
    RootWildcardPattern {
        pattern: String,
        #[source]
        source: glob::PatternError,
    },
    /// Invalid wildcard pattern in a prune spec.
    #[error("invalid prune wildcard pattern '{pattern}'")]
    PruneWildcardPattern {
        pattern: String,
        #[source]
        source: globset::Error,
    },
    /// I/O error while iterating wildcard matches.
    #[error("error reading wildcard match for pattern '{pattern}'")]
    WildcardIteration {
        pattern: String,
        #[source]
        source: glob::GlobError,
    },
    /// A required root path does not exist.
    #[error("required root path not found: '{}'", path.display())]
    RootPathNotFound { path: PathBuf },
    /// A required prune path does not exist.
    #[error("required prune path not found: '{}'", path.display())]
    PrunePathNotFound { path: PathBuf },
    /// A pattern with `must_exist=true` matched zero files.
    #[error("pattern matched no files: '{pattern}'")]
    WildcardMustExist { pattern: String },
    /// A path exists but is neither a file nor a directory.
    #[error("unsupported path type (not a file or directory): '{}'", path.display())]
    UnsupportedPathType { path: PathBuf },
    /// A symlink pointing to a directory was used as a root path.
    #[error("symlinked directory is not traversable: '{}'", path.display())]
    SymlinkDirNotTraversable { path: PathBuf },
    /// I/O error while reading file or symlink metadata.
    #[error("error reading metadata for '{}'", path.display())]
    MetadataIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// Base directory path contains invalid UTF-8.
    #[error(transparent)]
    BaseDirNotUtf8(PathError),
    /// I/O error while traversing a directory.
    #[error("error reading directory '{}'", path.display())]
    DirTraversalIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// Failed to build the prune wildcard matcher.
    #[error("failed to build prune matcher")]
    PruneMatcherBuild {
        #[source]
        source: globset::Error,
    },
    /// Invalid wildcard pattern in a `where` filter.
    #[error("invalid where.{attr} pattern '{pattern}'")]
    WherePatternInvalid {
        attr: &'static str,
        pattern: String,
        #[source]
        source: globset::Error,
    },
    /// A relative path was encountered but no base directory was provided.
    #[error("no base directory provided for relative path '{path}'")]
    MissingBaseDir { path: String },
}

// ---------------------------------------------------------------------------------------------- //
// Error Policy

/// Controls how I/O errors during directory traversal and wildcard expansion are handled.
///
/// This only affects errors encountered while walking directories (`read_dir`, reading
/// entries, `file_type`) and iterating wildcard matches. Structural errors like invalid
/// patterns, missing required paths, or unsupported path types always fail regardless.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OnError {
    /// Stops on the first I/O error.
    Fail,
    /// Logs a warning to stderr and continues.
    #[default]
    Warn,
    /// Silently skips the problematic entry and continues.
    Ignore,
}

impl OnError {
    /// Handles a traversal error according to this policy.
    fn handle(&self, err: FilesError) -> Result<(), FilesError> {
        match self {
            OnError::Fail => Err(err),
            OnError::Warn => {
                eprintln!("warning: {err}");
                Ok(())
            }
            OnError::Ignore => Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Parsing

impl FromNode for Files {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        if node.kind.is_leaf() {
            return Ok(Files {
                sources: vec![SourceSpec::from_node(node)?],
            });
        }

        if node.kind.is_vec() {
            return Ok(Files {
                sources: Vec::<SourceSpec>::from_node(node)?,
            });
        }

        if let Some(map) = node.as_opt_map() {
            if map.contains_key("sources") {
                let sources = node.req::<Vec<SourceSpec>>("sources")?;
                node.ensure_no_unknown_keys()?;
                return Ok(Files { sources });
            }

            return Ok(Files {
                sources: vec![SourceSpec::from_node(node)?],
            });
        }

        Err(NodeError::invalid_value(
            &node.path,
            "expected string, array, source map, or map with 'sources'",
        ))
    }
}

impl Describe for Files {
    fn describe() -> Desc {
        Desc::structure(vec![desc::FieldDesc {
            name: "sources".to_string(),
            doc: "The sources to collect files from.".to_string(),
            optional: false,
            default_expr: None,
            value: Desc::plain("file source…"),
        }])
    }
}

impl FromNode for SourceSpec {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        // String shorthand: becomes a single from.root entry.
        if node.kind.is_leaf() {
            let pp = PathPatternSpec::from_node(node)?;
            return Ok(SourceSpec {
                from: Some(FromSpec {
                    root: OneOrMany::one(pp),
                    prune: OneOrMany::default(),
                }),
                where_: None,
            });
        }

        if node.kind.is_map() {
            // Parse optional `from` block.
            let from = match node.opt_node("from")? {
                Some(from_node) => Some(FromSpec::from_node(from_node)?),
                None => None,
            };

            // Parse optional `where` block.
            let where_ = match node.opt_node("where")? {
                Some(where_node) => Some(WhereSpec::from_node(where_node)?),
                None => None,
            };

            node.ensure_no_unknown_keys()?;
            return Ok(SourceSpec { from, where_ });
        }

        Err(NodeError::invalid_value(
            &node.path,
            "expected string or map with optional 'from'/'where' keys",
        ))
    }
}

impl Describe for SourceSpec {
    fn describe() -> Desc {
        Desc::plain("one source: where to search and which files to keep")
    }
}

impl FromNode for FromSpec {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        // String shorthand: becomes a single root entry.
        if node.kind.is_leaf() {
            let pp = PathPatternSpec::from_node(node)?;
            return Ok(FromSpec {
                root: OneOrMany::one(pp),
                prune: OneOrMany::default(),
            });
        }

        // Array shorthand: becomes a root list.
        if node.kind.is_vec() {
            let root: OneOrMany<PathPatternSpec> = node.as_type()?;
            return Ok(FromSpec {
                root,
                prune: OneOrMany::default(),
            });
        }

        if node.kind.is_map() {
            let root: OneOrMany<PathPatternSpec> = node.opt("root")?.unwrap_or_default();
            let prune: OneOrMany<PathPatternSpec> = node.opt("prune")?.unwrap_or_default();
            node.ensure_no_unknown_keys()?;
            return Ok(FromSpec { root, prune });
        }

        Err(NodeError::invalid_value(
            &node.path,
            "expected string, array, or map with 'root'/'prune' keys",
        ))
    }
}

impl Describe for FromSpec {
    fn describe() -> Desc {
        Desc::plain("where to search and which paths to skip")
    }
}

impl FromNode for PathPatternSpec {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        // String shorthand: auto-detect syntax from wildcards.
        if node.kind.is_leaf() {
            let s: String = node.as_type()?;
            return Ok(auto_detect_path_pattern(s));
        }

        // Explicit map form.
        if node.kind.is_map() {
            let path: String = node.req("path")?;
            let syntax: PatternSyntax = match node.opt("syntax")? {
                Some(syntax) => syntax,
                None => auto_detect_path_syntax(&path),
            };
            let default_must_exist = syntax == PatternSyntax::Exact;
            let must_exist: bool = node.opt("must_exist")?.unwrap_or(default_must_exist);
            let recursive: bool = node.opt("recursive")?.unwrap_or(true);
            node.ensure_no_unknown_keys()?;
            return Ok(PathPatternSpec {
                path,
                syntax,
                must_exist,
                recursive,
            });
        }

        Err(NodeError::invalid_value(&node.path, "expected string or map with 'path' key"))
    }
}

impl Describe for PathPatternSpec {
    fn describe() -> Desc {
        Desc::plain("path pattern")
    }
}

impl FromNode for PatternSyntax {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        let s: String = node.as_type()?;
        match s.as_str() {
            "exact" => Ok(PatternSyntax::Exact),
            "wildcard" => Ok(PatternSyntax::Wildcard),
            _ => Err(NodeError::invalid_value(&node.path, "expected \"exact\" or \"wildcard\"")),
        }
    }
}

impl Describe for PatternSyntax {
    fn describe() -> Desc {
        Desc::plain("pattern syntax")
    }
}

impl FromNode for WhereSpec {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        let case: CaseMode = node.opt("case")?.unwrap_or_default();

        let path = match node.opt_node("path")? {
            Some(n) => Some(AttrRuleSpec::from_node(n)?),
            None => None,
        };
        let name = match node.opt_node("name")? {
            Some(n) => Some(AttrRuleSpec::from_node(n)?),
            None => None,
        };
        let stem = match node.opt_node("stem")? {
            Some(n) => Some(AttrRuleSpec::from_node(n)?),
            None => None,
        };
        let ext = match node.opt_node("ext")? {
            Some(n) => Some(AttrRuleSpec::from_node(n)?),
            None => None,
        };

        node.ensure_no_unknown_keys()?;
        Ok(WhereSpec {
            case,
            path,
            name,
            stem,
            ext,
        })
    }
}

impl Describe for WhereSpec {
    fn describe() -> Desc {
        Desc::plain("path, name, stem, and ext filters")
    }
}

impl FromNode for CaseMode {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        let s: String = node.as_type()?;
        match s.as_str() {
            "insensitive" => Ok(CaseMode::Insensitive),
            "sensitive" => Ok(CaseMode::Sensitive),
            _ => Err(NodeError::invalid_value(
                &node.path,
                "expected \"insensitive\" or \"sensitive\"",
            )),
        }
    }
}

impl Describe for CaseMode {
    fn describe() -> Desc {
        Desc::plain("case mode")
    }
}

impl FromNode for AttrRuleSpec {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        // String shorthand: single pattern as include-only.
        if node.kind.is_leaf() {
            let tp = TextPatternSpec::from_node(node)?;
            return Ok(AttrRuleSpec {
                include: OneOrMany::one(tp),
                exclude: OneOrMany::default(),
            });
        }

        // Array shorthand: list of patterns as include-only.
        if node.kind.is_vec() {
            let include: Vec<TextPatternSpec> = node.as_type()?;
            return Ok(AttrRuleSpec {
                include: OneOrMany::new(include),
                exclude: OneOrMany::default(),
            });
        }

        // Map form: { include: [...], exclude: [...] }
        if node.kind.is_map() {
            let include: OneOrMany<TextPatternSpec> = node.opt("include")?.unwrap_or_default();
            let exclude: OneOrMany<TextPatternSpec> = node.opt("exclude")?.unwrap_or_default();
            node.ensure_no_unknown_keys()?;
            return Ok(AttrRuleSpec { include, exclude });
        }

        Err(NodeError::invalid_value(
            &node.path,
            "expected string, array, or map with 'include'/'exclude'",
        ))
    }
}

impl Describe for AttrRuleSpec {
    fn describe() -> Desc {
        Desc::plain("include/exclude rule")
    }
}

impl FromNode for TextPatternSpec {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        // String shorthand: auto-detect syntax from wildcards.
        if node.kind.is_leaf() {
            let s: String = node.as_type()?;
            return Ok(auto_detect_text_pattern(s));
        }

        // Explicit map form.
        if node.kind.is_map() {
            let pattern: String = node.req("pattern")?;
            let syntax: PatternSyntax = match node.opt("syntax")? {
                Some(syntax) => syntax,
                None => auto_detect_text_syntax(&pattern),
            };
            node.ensure_no_unknown_keys()?;
            return Ok(TextPatternSpec { pattern, syntax });
        }

        Err(NodeError::invalid_value(&node.path, "expected string or map with 'pattern' key"))
    }
}

impl Describe for TextPatternSpec {
    fn describe() -> Desc {
        Desc::plain("text pattern")
    }
}

// ---------------------------------------------------------------------------------------------- //
// Resolution

impl Files {
    /// Collects file paths from all sources.
    ///
    /// Iterates over each [`SourceSpec`], calls its `locate`, and deduplicates results
    /// across sources while preserving first-seen order.
    pub fn locate(&self, base_dir: Option<&Path>) -> Result<Vec<PathBuf>, FilesError> {
        self.locate_with(base_dir, OnError::default())
    }

    /// Collects file paths from all sources, with explicit error handling policy.
    ///
    /// Like [`locate`](Self::locate), but lets the caller choose how I/O errors during
    /// directory traversal and wildcard expansion are handled.
    pub fn locate_with(
        &self,
        base_dir: Option<&Path>,
        on_error: OnError,
    ) -> Result<Vec<PathBuf>, FilesError> {
        let mut all_files: IndexSet<PathBuf> = IndexSet::new();
        for source in &self.sources {
            let files = source.locate_with(base_dir, on_error)?;
            all_files.extend(files);
        }
        Ok(all_files.into_iter().collect())
    }
}

impl SourceSpec {
    /// Collects file paths matching this source's specification.
    ///
    /// If `base_dir` is `Some`, relative paths and patterns are resolved against it.
    /// If `None`, all paths must be absolute. If `from` is omitted, `base_dir` is used
    /// as the implicit root.
    pub fn locate(&self, base_dir: Option<&Path>) -> Result<Vec<PathBuf>, FilesError> {
        self.locate_with(base_dir, OnError::default())
    }

    /// Collects file paths matching this source's specification, with explicit error handling
    /// policy.
    pub fn locate_with(
        &self,
        base_dir: Option<&Path>,
        on_error: OnError,
    ) -> Result<Vec<PathBuf>, FilesError> {
        // Determine effective from spec. If omitted, use base_dir as implicit root.
        let default_from;
        let from = match &self.from {
            Some(f) => f,
            None => {
                let base = base_dir.ok_or_else(|| FilesError::MissingBaseDir {
                    path: "<implicit root>".to_string(),
                })?;
                default_from = FromSpec {
                    root: OneOrMany::one(PathPatternSpec {
                        path: base.to_string_lossy().into_owned(),
                        syntax: PatternSyntax::Exact,
                        must_exist: true,
                        recursive: true,
                    }),
                    prune: OneOrMany::default(),
                };
                &default_from
            }
        };

        // If root is empty, also default to base_dir.
        let default_root_from;
        let effective_from = if from.root.is_empty() {
            let base = base_dir.ok_or_else(|| FilesError::MissingBaseDir {
                path: "<implicit root>".to_string(),
            })?;
            default_root_from = FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: base.to_string_lossy().into_owned(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: from.prune.clone(),
            };
            &default_root_from
        } else {
            from
        };

        // Compile prune rules.
        let compiled_prune = CompiledPrune::build(base_dir, &effective_from.prune)?;

        // Compile where spec (if present).
        let compiled_where = self.where_.as_ref().map(CompiledWhereSpec::build).transpose()?;

        // Expand all roots.
        let mut files: IndexSet<PathBuf> = IndexSet::new();
        for root_pattern in effective_from.root.iter() {
            expand_root(
                root_pattern,
                base_dir,
                &mut files,
                compiled_where.as_ref(),
                &compiled_prune,
                on_error,
            )?;
        }

        // Apply post-collection prune filters on the collected files.
        let result: Vec<PathBuf> =
            files.into_iter().filter(|path| !compiled_prune.is_excluded(path, base_dir)).collect();

        Ok(result)
    }
}

// ---------------------------------------------------------------------------------------------- //
// Compiled Prune

/// Pre-compiled prune matchers built from `from.prune` patterns.
///
/// Exact patterns are split by type: files go into `exclude_files`, directories into
/// `prune_dirs` (traversal-time pruning) or `exclude_dirs_nonrec`. Wildcard patterns
/// go into `prune_globset` and are matched post-collection only - they cannot prevent
/// directory traversal.
struct CompiledPrune {
    /// Exact file paths to exclude.
    exclude_files: HashSet<PathBuf>,
    /// Directories to prune recursively (recursive=true).
    prune_dirs: Vec<PathBuf>,
    /// Directories to exclude direct children from (recursive=false).
    exclude_dirs_nonrec: Vec<PathBuf>,
    /// Compiled wildcard patterns for prune matching.
    prune_globset: GlobSet,
}

impl CompiledPrune {
    /// Builds compiled prune rules from a list of prune patterns.
    fn build(
        base_dir: Option<&Path>,
        patterns: &[PathPatternSpec],
    ) -> Result<CompiledPrune, FilesError> {
        let mut exclude_files = HashSet::new();
        let mut prune_dirs = Vec::new();
        let mut exclude_dirs_nonrec = Vec::new();
        let mut glob_builder = GlobSetBuilder::new();

        for pp in patterns {
            match pp.syntax {
                PatternSyntax::Exact => {
                    let resolved = adapt_path(base_dir, Path::new(&pp.path))?;
                    if !resolved.exists() {
                        if pp.must_exist {
                            return Err(FilesError::PrunePathNotFound { path: resolved });
                        }
                        continue;
                    }
                    // Use metadata (follows symlinks) so symlinked dirs can be pruned.
                    let meta =
                        std::fs::metadata(&resolved).map_err(|e| FilesError::MetadataIo {
                            path: resolved.clone(),
                            source: e,
                        })?;
                    if meta.is_dir() {
                        if pp.recursive {
                            prune_dirs.push(resolved);
                        } else {
                            exclude_dirs_nonrec.push(resolved);
                        }
                    } else if meta.is_file() {
                        exclude_files.insert(resolved);
                    } else {
                        return Err(FilesError::UnsupportedPathType { path: resolved });
                    }
                }
                PatternSyntax::Wildcard => {
                    let escaped = escape_wildcard_for_globset(&pp.path);
                    let glob = GlobBuilder::new(&escaped).literal_separator(true).build().map_err(
                        |e| FilesError::PruneWildcardPattern {
                            pattern: pp.path.clone(),
                            source: e,
                        },
                    )?;
                    glob_builder.add(glob);
                }
            }
        }

        let prune_globset =
            glob_builder.build().map_err(|e| FilesError::PruneMatcherBuild { source: e })?;

        Ok(CompiledPrune {
            exclude_files,
            prune_dirs,
            exclude_dirs_nonrec,
            prune_globset,
        })
    }

    /// Returns true if the given path should be excluded from results.
    fn is_excluded(&self, path: &Path, base_dir: Option<&Path>) -> bool {
        // Exact file match.
        if self.exclude_files.contains(path) {
            return true;
        }
        // Recursive directory prefix match.
        for dir in &self.prune_dirs {
            if path.starts_with(dir) {
                return true;
            }
        }
        // Non-recursive directory: only direct children.
        for dir in &self.exclude_dirs_nonrec {
            if path.parent() == Some(dir.as_path()) {
                return true;
            }
        }
        // Wildcard prune: try the full path first, then the base_dir-relative path.
        if !self.prune_globset.is_empty() {
            if self.prune_globset.is_match(path) {
                return true;
            }
            if let Some(bd) = base_dir {
                if let Ok(relative) = path.strip_prefix(bd) {
                    if self.prune_globset.is_match(relative) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Returns true if a directory should be pruned during traversal.
    fn is_pruned(&self, dir: &Path) -> bool {
        self.prune_dirs.iter().any(|pd| dir.starts_with(pd))
    }
}

// ---------------------------------------------------------------------------------------------- //
// Root Expansion

/// Expands a single root pattern and adds matching files to the set.
fn expand_root(
    pp: &PathPatternSpec,
    base_dir: Option<&Path>,
    files: &mut IndexSet<PathBuf>,
    where_filter: Option<&CompiledWhereSpec>,
    compiled_prune: &CompiledPrune,
    on_error: OnError,
) -> Result<(), FilesError> {
    match pp.syntax {
        PatternSyntax::Exact => {
            let resolved = adapt_path(base_dir, Path::new(&pp.path))?;
            if !resolved.exists() {
                if pp.must_exist {
                    return Err(FilesError::RootPathNotFound { path: resolved });
                }
                return Ok(());
            }
            match classify_path(&resolved)? {
                Some(ResolvedPath::File) => {
                    let source_root = resolved.parent().unwrap_or_else(|| Path::new(""));
                    if where_filter.is_none_or(|f| f.matches(&resolved, source_root)) {
                        files.insert(resolved);
                    }
                }
                Some(ResolvedPath::Dir) => {
                    // Skip the walk entirely if the root is pruned.
                    if !compiled_prune.is_pruned(&resolved) {
                        let source_root = resolved.as_path();
                        let mut batch: Vec<PathBuf> = Vec::new();
                        collect_dir_files(
                            &resolved,
                            source_root,
                            pp.recursive,
                            &mut batch,
                            where_filter,
                            compiled_prune,
                            on_error,
                        )?;
                        batch.sort();
                        for path in batch {
                            files.insert(path);
                        }
                    }
                }
                None => {
                    // Broken symlink: skip silently.
                }
            }
        }
        PatternSyntax::Wildcard => {
            let adapted = adapt_pattern(base_dir, &pp.path)?;
            let compiled = escape_for_glob(&adapted);
            let mut batch = expand_wildcard(
                &compiled,
                &pp.path,
                pp.recursive,
                where_filter,
                compiled_prune,
                on_error,
            )?;
            if pp.must_exist && batch.is_empty() {
                return Err(FilesError::WildcardMustExist {
                    pattern: pp.path.clone(),
                });
            }
            batch.sort();
            for path in batch {
                files.insert(path);
            }
        }
    }
    Ok(())
}

/// Expands a wildcard pattern and returns matching files.
///
/// File matches are filtered through `where_filter` and collected directly. Directory
/// matches are walked using [`collect_dir_files`], respecting `recursive` and `prune`.
/// Symlinked directories matched by the wildcard are treated as errors (consistent with
/// exact root handling via [`classify_path`]).
fn expand_wildcard(
    pattern: &str,
    original_pattern: &str,
    recursive: bool,
    where_filter: Option<&CompiledWhereSpec>,
    compiled_prune: &CompiledPrune,
    on_error: OnError,
) -> Result<Vec<PathBuf>, FilesError> {
    let matches = glob::glob(pattern).map_err(|e| FilesError::RootWildcardPattern {
        pattern: original_pattern.to_string(),
        source: e,
    })?;

    let mut batch: Vec<PathBuf> = Vec::new();
    for result in matches {
        let path = match result {
            Ok(path) => path,
            Err(e) => {
                on_error.handle(FilesError::WildcardIteration {
                    pattern: original_pattern.to_string(),
                    source: e,
                })?;
                continue;
            }
        };
        match classify_path(&path)? {
            Some(ResolvedPath::File) => {
                let source_root = path.parent().unwrap_or_else(|| Path::new(""));
                if where_filter.is_none_or(|f| f.matches(&path, source_root)) {
                    batch.push(path);
                }
            }
            Some(ResolvedPath::Dir) => {
                if !compiled_prune.is_pruned(&path) {
                    let source_root = path.as_path();
                    collect_dir_files(
                        &path,
                        source_root,
                        recursive,
                        &mut batch,
                        where_filter,
                        compiled_prune,
                        on_error,
                    )?;
                }
            }
            None => {
                // Broken symlink: skip silently.
            }
        }
    }

    Ok(batch)
}

// ---------------------------------------------------------------------------------------------- //
// Directory Traversal

/// Classifies a resolved path for use by root expansion.
enum ResolvedPath {
    File,
    Dir,
}

/// Classifies a resolved path, returning an error for unsupported types.
///
/// Returns `Ok(None)` for broken symlinks (caller decides whether to skip or error).
fn classify_path(resolved: &Path) -> Result<Option<ResolvedPath>, FilesError> {
    let link_meta = std::fs::symlink_metadata(resolved).map_err(|e| FilesError::MetadataIo {
        path: resolved.to_path_buf(),
        source: e,
    })?;

    if link_meta.file_type().is_symlink() {
        let target_meta = match std::fs::metadata(resolved) {
            Ok(m) => m,
            Err(_) => return Ok(None), // Broken symlink.
        };
        if target_meta.is_file() {
            Ok(Some(ResolvedPath::File))
        } else if target_meta.is_dir() {
            Err(FilesError::SymlinkDirNotTraversable {
                path: resolved.to_path_buf(),
            })
        } else {
            Err(FilesError::UnsupportedPathType {
                path: resolved.to_path_buf(),
            })
        }
    } else if link_meta.is_file() {
        Ok(Some(ResolvedPath::File))
    } else if link_meta.is_dir() {
        Ok(Some(ResolvedPath::Dir))
    } else {
        Err(FilesError::UnsupportedPathType {
            path: resolved.to_path_buf(),
        })
    }
}

/// Recursively collects files from a directory into a batch.
///
/// Symlinked files are included if their target is a file. Symlinked directories are
/// never recursed into, to avoid loops and surprises. Broken symlinks are silently skipped.
/// Pruned directories and files are skipped.
fn collect_dir_files(
    dir: &Path,
    source_root: &Path,
    recursive: bool,
    batch: &mut Vec<PathBuf>,
    where_filter: Option<&CompiledWhereSpec>,
    compiled_prune: &CompiledPrune,
    on_error: OnError,
) -> Result<(), FilesError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            return on_error.handle(FilesError::DirTraversalIo {
                path: dir.to_path_buf(),
                source: e,
            });
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                on_error.handle(FilesError::DirTraversalIo {
                    path: dir.to_path_buf(),
                    source: e,
                })?;
                continue;
            }
        };
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                on_error.handle(FilesError::DirTraversalIo {
                    path: entry.path(),
                    source: e,
                })?;
                continue;
            }
        };

        if ft.is_file() {
            let path = entry.path();
            if where_filter.is_none_or(|f| f.matches(&path, source_root)) {
                batch.push(path);
            }
        } else if recursive && ft.is_dir() {
            let path = entry.path();
            // Check if this directory should be pruned.
            if compiled_prune.is_pruned(&path) {
                continue;
            }
            collect_dir_files(
                &path,
                source_root,
                true,
                batch,
                where_filter,
                compiled_prune,
                on_error,
            )?;
        } else if ft.is_symlink() {
            let target_meta = match std::fs::metadata(entry.path()) {
                Ok(meta) => meta,
                Err(_) => continue, // Broken symlink: skip silently.
            };
            if target_meta.is_file() {
                let path = entry.path();
                if where_filter.is_none_or(|f| f.matches(&path, source_root)) {
                    batch.push(path);
                }
            }
            // Symlinked directories are intentionally not recursed into.
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------------------------- //
// Where Matching Engine

/// Pre-compiled where-spec for efficient matching during resolution.
struct CompiledWhereSpec {
    /// Compiled path filter (if present).
    path: Option<CompiledPathRule>,
    /// Compiled file name filter (if present).
    name: Option<CompiledAttrRule>,
    /// Compiled stem filter (if present).
    stem: Option<CompiledAttrRule>,
    /// Compiled ext filter (if present).
    ext: Option<CompiledAttrRule>,
}

/// Pre-compiled include/exclude lists for root-relative or absolute paths.
struct CompiledPathRule {
    /// Include patterns. If empty, all paths are allowed.
    include: Vec<CompiledPathPattern>,
    /// Exclude patterns. Exclude wins over include.
    exclude: Vec<CompiledPathPattern>,
}

/// A single pre-compiled path pattern ready for matching.
enum CompiledPathPattern {
    /// Exact path text comparison.
    Exact {
        pattern: String,
        absolute: bool,
        case_insensitive: bool,
    },
    /// Pre-compiled wildcard matcher.
    Wildcard {
        matcher: GlobMatcher,
        absolute: bool,
    },
}

/// Pre-compiled include/exclude lists for a single attribute.
struct CompiledAttrRule {
    /// Include patterns. If empty, all values are allowed.
    include: Vec<CompiledTextPattern>,
    /// Exclude patterns. Exclude wins over include.
    exclude: Vec<CompiledTextPattern>,
}

/// A single pre-compiled text pattern ready for matching.
///
/// For `Exact` patterns, stores the raw pattern string and a case-sensitivity flag.
/// For `Wildcard` patterns, stores a pre-compiled matcher with case sensitivity baked in.
enum CompiledTextPattern {
    /// Exact string comparison.
    Exact {
        pattern: String,
        case_insensitive: bool,
    },
    /// Pre-compiled wildcard matcher.
    Wildcard(GlobMatcher),
}

impl CompiledWhereSpec {
    /// Builds a compiled where-spec from a [`WhereSpec`].
    ///
    /// Compiles all patterns up front.
    fn build(spec: &WhereSpec) -> Result<Self, FilesError> {
        let ci = spec.case == CaseMode::Insensitive;
        let path = spec.path.as_ref().map(|s| CompiledPathRule::build(s, ci)).transpose()?;
        let name = spec
            .name
            .as_ref()
            .map(|s| CompiledAttrRule::build(s, ci, "name", false))
            .transpose()?;
        let stem = spec
            .stem
            .as_ref()
            .map(|s| CompiledAttrRule::build(s, ci, "stem", false))
            .transpose()?;
        let ext =
            spec.ext.as_ref().map(|s| CompiledAttrRule::build(s, ci, "ext", true)).transpose()?;
        Ok(Self {
            path,
            name,
            stem,
            ext,
        })
    }

    /// Returns true if the path passes all filters.
    fn matches(&self, path: &Path, source_root: &Path) -> bool {
        if let Some(ref rule) = self.path {
            if !rule.matches(path, source_root) {
                return false;
            }
        }

        // Check name filter.
        if let Some(ref rule) = self.name {
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => return false, // Non-UTF8 file name: treat as non-matching.
            };
            if !rule.matches(name) {
                return false;
            }
        }

        // Check stem filter.
        if let Some(ref rule) = self.stem {
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => return false,
            };
            if !rule.matches(stem) {
                return false;
            }
        }

        // Check ext filter.
        if let Some(ref rule) = self.ext {
            // Extension can be None (file without extension). Represent as "".
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !rule.matches(ext) {
                return false;
            }
        }

        true
    }
}

impl CompiledPathRule {
    /// Builds a compiled path rule from an [`AttrRuleSpec`].
    fn build(spec: &AttrRuleSpec, case_insensitive: bool) -> Result<Self, FilesError> {
        let build_one = |p: &TextPatternSpec| CompiledPathPattern::build(p, case_insensitive);
        let include = spec.include.iter().map(build_one).collect::<Result<Vec<_>, _>>()?;
        let exclude = spec.exclude.iter().map(build_one).collect::<Result<Vec<_>, _>>()?;
        Ok(Self { include, exclude })
    }

    /// Returns true if the path passes the include/exclude filters.
    fn matches(&self, path: &Path, source_root: &Path) -> bool {
        if self.exclude.iter().any(|p| p.matches(path, source_root)) {
            return false;
        }
        if self.include.is_empty() {
            return true;
        }
        self.include.iter().any(|p| p.matches(path, source_root))
    }
}

impl CompiledPathPattern {
    /// Builds a compiled path pattern.
    fn build(pattern: &TextPatternSpec, case_insensitive: bool) -> Result<Self, FilesError> {
        let absolute = Path::new(&pattern.pattern).is_absolute();
        let normalized = normalize_path_separators(&pattern.pattern);

        match pattern.syntax {
            PatternSyntax::Exact => Ok(Self::Exact {
                pattern: normalized,
                absolute,
                case_insensitive,
            }),
            PatternSyntax::Wildcard => {
                let escaped = escape_wildcard_for_globset(&normalized);
                let glob = GlobBuilder::new(&escaped)
                    .case_insensitive(case_insensitive)
                    .literal_separator(true)
                    .build()
                    .map_err(|e| FilesError::WherePatternInvalid {
                        attr: "path",
                        pattern: pattern.pattern.clone(),
                        source: e,
                    })?;
                Ok(Self::Wildcard {
                    matcher: glob.compile_matcher(),
                    absolute,
                })
            }
        }
    }

    /// Returns true if the candidate path matches this pattern.
    fn matches(&self, path: &Path, source_root: &Path) -> bool {
        let candidate = match self {
            Self::Exact { absolute, .. } | Self::Wildcard { absolute, .. } => {
                candidate_path_text(path, source_root, *absolute)
            }
        };
        let Some(candidate) = candidate else {
            return false;
        };

        match self {
            Self::Exact {
                pattern,
                case_insensitive,
                ..
            } => {
                if *case_insensitive {
                    candidate.eq_ignore_ascii_case(pattern)
                } else {
                    candidate == pattern.as_str()
                }
            }
            Self::Wildcard { matcher, .. } => matcher.is_match(candidate),
        }
    }
}

impl CompiledAttrRule {
    /// Builds a compiled attribute rule from an [`AttrRuleSpec`].
    ///
    /// If `normalize_ext` is true, leading dots are stripped from pattern strings before
    /// compilation (used for extension patterns where `".rs"` and `"rs"` should be equivalent).
    fn build(
        spec: &AttrRuleSpec,
        case_insensitive: bool,
        attr: &'static str,
        normalize_ext: bool,
    ) -> Result<Self, FilesError> {
        let build_one = |p: &TextPatternSpec| {
            let pattern = if normalize_ext {
                normalize_extension(&p.pattern).to_string()
            } else {
                p.pattern.clone()
            };
            CompiledTextPattern::build(&pattern, p.syntax, case_insensitive, attr)
        };
        let include = spec.include.iter().map(build_one).collect::<Result<Vec<_>, _>>()?;
        let exclude = spec.exclude.iter().map(build_one).collect::<Result<Vec<_>, _>>()?;
        Ok(Self { include, exclude })
    }

    /// Returns true if the value passes the include/exclude filters.
    fn matches(&self, value: &str) -> bool {
        // Exclude wins over include.
        if self.exclude.iter().any(|p| p.matches(value)) {
            return false;
        }
        // If include is empty, all values pass. Otherwise, must match at least one.
        if self.include.is_empty() {
            return true;
        }
        self.include.iter().any(|p| p.matches(value))
    }
}

impl CompiledTextPattern {
    /// Builds a compiled text pattern from a pattern string, syntax, and case mode.
    ///
    /// For `Exact` patterns, stores the string and a case-sensitivity flag.
    /// For `Wildcard` patterns, compiles via `GlobBuilder` with the case mode baked in.
    fn build(
        pattern: &str,
        syntax: PatternSyntax,
        case_insensitive: bool,
        attr: &'static str,
    ) -> Result<Self, FilesError> {
        match syntax {
            PatternSyntax::Exact => Ok(Self::Exact {
                pattern: pattern.to_string(),
                case_insensitive,
            }),
            PatternSyntax::Wildcard => {
                let escaped = escape_wildcard_for_globset(pattern);
                let glob = GlobBuilder::new(&escaped)
                    .case_insensitive(case_insensitive)
                    .literal_separator(false)
                    .build()
                    .map_err(|e| FilesError::WherePatternInvalid {
                        attr,
                        pattern: pattern.to_string(),
                        source: e,
                    })?;
                Ok(Self::Wildcard(glob.compile_matcher()))
            }
        }
    }

    /// Returns true if the value matches this pattern.
    fn matches(&self, value: &str) -> bool {
        match self {
            Self::Exact {
                pattern,
                case_insensitive,
            } => {
                if *case_insensitive {
                    value.eq_ignore_ascii_case(pattern)
                } else {
                    value == pattern.as_str()
                }
            }
            Self::Wildcard(matcher) => matcher.is_match(value),
        }
    }
}

// ---------------------------------------------------------------------------------------------- //
// Helper Functions

/// Auto-detects a path pattern from a plain string.
///
/// If the string contains `*` or `?`, it becomes a `Wildcard` pattern with `must_exist=false`.
/// Otherwise, it becomes an `Exact` pattern with `must_exist=true` and `recursive=true`.
fn auto_detect_path_pattern(s: String) -> PathPatternSpec {
    let syntax = auto_detect_path_syntax(&s);
    if syntax == PatternSyntax::Wildcard {
        PathPatternSpec {
            path: s,
            syntax,
            must_exist: false,
            recursive: true,
        }
    } else {
        PathPatternSpec {
            path: s,
            syntax,
            must_exist: true,
            recursive: true,
        }
    }
}

/// Auto-detects a text pattern from a plain string.
///
/// If the string contains `*` or `?`, it becomes a `Wildcard` pattern.
/// Otherwise, it becomes an `Exact` pattern.
fn auto_detect_text_pattern(s: String) -> TextPatternSpec {
    let syntax = auto_detect_text_syntax(&s);
    TextPatternSpec { pattern: s, syntax }
}

/// Auto-detects path pattern syntax from wildcard characters.
fn auto_detect_path_syntax(s: &str) -> PatternSyntax {
    if contains_wildcard(s) {
        PatternSyntax::Wildcard
    } else {
        PatternSyntax::Exact
    }
}

/// Auto-detects text pattern syntax from wildcard characters.
fn auto_detect_text_syntax(s: &str) -> PatternSyntax {
    if contains_wildcard(s) {
        PatternSyntax::Wildcard
    } else {
        PatternSyntax::Exact
    }
}

/// Checks if a pattern string contains wildcard characters.
fn contains_wildcard(s: &str) -> bool {
    s.contains('*') || s.contains('?')
}

/// Normalizes backslash path separators to forward slashes.
///
/// Used to convert Windows filesystem paths to `/`-separated strings before joining
/// them with wildcard patterns. On non-Windows platforms, this is a no-op since backslash
/// is a legal filename character.
fn normalize_path_separators(s: &str) -> String {
    if cfg!(windows) {
        s.replace('\\', "/")
    } else {
        s.to_string()
    }
}

/// Adapts a path to a base directory if needed.
fn adapt_path(base_dir: Option<&Path>, path: &Path) -> Result<PathBuf, FilesError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    match base_dir {
        Some(dir) => Ok(dir.join(path)),
        None => Err(FilesError::MissingBaseDir {
            path: path.display().to_string(),
        }),
    }
}

/// Adapts a wildcard pattern string to a base directory if needed.
///
/// Patterns are treated as UTF-8 strings with `/` as the canonical separator.
/// On Windows, backslashes in the base directory path are normalized to `/`.
/// The wildcard pattern itself is never modified (use `/` for separators in patterns).
fn adapt_pattern(base_dir: Option<&Path>, pattern: &str) -> Result<String, FilesError> {
    let path = Path::new(pattern);
    if path.is_absolute() {
        return Ok(pattern.to_string());
    }
    match base_dir {
        Some(dir) => {
            let mut base =
                normalize_path_separators(dir.path_str().map_err(FilesError::BaseDirNotUtf8)?);
            if !base.ends_with('/') {
                base.push('/');
            }
            Ok(format!("{base}{pattern}"))
        }
        None => Err(FilesError::MissingBaseDir {
            path: pattern.to_string(),
        }),
    }
}

/// Escapes a wildcard pattern for the `glob` crate.
fn escape_for_glob(pattern: &str) -> String {
    escape_wildcard_for_globset(pattern)
}

/// Escapes a wildcard pattern for use with `globset`.
///
/// `*`, `?`, and `**` keep their wildcard meaning. Braces and brackets are escaped so they are
/// treated as literal characters.
fn escape_wildcard_for_globset(pattern: &str) -> String {
    let needs_work = pattern.contains('{')
        || pattern.contains('}')
        || pattern.contains('[')
        || pattern.contains(']');
    if !needs_work {
        return pattern.to_string();
    }

    let mut result = String::with_capacity(pattern.len() + 16);
    for c in pattern.chars() {
        match c {
            '{' => result.push_str("[{]"),
            '}' => result.push_str("[}]"),
            '[' => result.push_str("[[]"),
            ']' => result.push_str("[]]"),
            _ => result.push(c),
        }
    }
    result
}

/// Converts a candidate path to the text used for path pattern matching.
fn candidate_path_text(path: &Path, source_root: &Path, absolute: bool) -> Option<String> {
    let candidate = if absolute {
        path
    } else {
        path.strip_prefix(source_root).ok()?
    };
    candidate.to_str().map(normalize_path_separators)
}

/// Normalizes an extension string by removing a leading dot.
fn normalize_extension(ext: &str) -> &str {
    ext.strip_prefix('.').unwrap_or(ext)
}

// ---------------------------------------------------------------------------------------------- //

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Format;
    use std::fs;
    use tempfile::TempDir;

    // ------------------------------------------------------------------------------------------ //
    // PathPatternSpec Parsing Tests

    #[test]
    fn path_pattern_string_no_wildcards() {
        let node = Node::parse_str("\"some/file.txt\"", Format::Rhai).unwrap();
        let pp: PathPatternSpec = node.as_type().unwrap();
        assert_eq!(pp.path, "some/file.txt");
        assert_eq!(pp.syntax, PatternSyntax::Exact);
        assert!(pp.must_exist);
        assert!(pp.recursive);
    }

    #[test]
    fn path_pattern_string_with_asterisk() {
        let node = Node::parse_str("\"*.txt\"", Format::Rhai).unwrap();
        let pp: PathPatternSpec = node.as_type().unwrap();
        assert_eq!(pp.path, "*.txt");
        assert_eq!(pp.syntax, PatternSyntax::Wildcard);
        assert!(!pp.must_exist);
    }

    #[test]
    fn path_pattern_string_with_question_mark() {
        let node = Node::parse_str("\"file?.txt\"", Format::Rhai).unwrap();
        let pp: PathPatternSpec = node.as_type().unwrap();
        assert_eq!(pp.syntax, PatternSyntax::Wildcard);
    }

    #[test]
    fn path_pattern_explicit_exact_defaults() {
        let node = Node::parse_str(r#"#{ path: "some/file.txt", syntax: "exact" }"#, Format::Rhai)
            .unwrap();
        let pp: PathPatternSpec = node.as_type().unwrap();
        assert_eq!(pp.path, "some/file.txt");
        assert_eq!(pp.syntax, PatternSyntax::Exact);
        assert!(pp.must_exist); // Default for exact.
        assert!(pp.recursive);
    }

    #[test]
    fn path_pattern_explicit_defaults_to_exact() {
        let node = Node::parse_str(r#"#{ path: "some/file.txt" }"#, Format::Rhai).unwrap();
        let pp: PathPatternSpec = node.as_type().unwrap();
        assert_eq!(pp.path, "some/file.txt");
        assert_eq!(pp.syntax, PatternSyntax::Exact);
        assert!(pp.must_exist); // Default for exact.
        assert!(pp.recursive);
    }

    #[test]
    fn path_pattern_explicit_wildcard_defaults() {
        let node =
            Node::parse_str(r#"#{ path: "**/*.rs", syntax: "wildcard" }"#, Format::Rhai).unwrap();
        let pp: PathPatternSpec = node.as_type().unwrap();
        assert_eq!(pp.syntax, PatternSyntax::Wildcard);
        assert!(!pp.must_exist); // Default for wildcard.
    }

    #[test]
    fn path_pattern_explicit_overrides() {
        let node = Node::parse_str(
            r#"#{ path: "src", syntax: "exact", must_exist: false, recursive: false }"#,
            Format::Rhai,
        )
        .unwrap();
        let pp: PathPatternSpec = node.as_type().unwrap();
        assert!(!pp.must_exist);
        assert!(!pp.recursive);
    }

    #[test]
    fn path_pattern_map_unknown_key_errors() {
        let node =
            Node::parse_str(r#"#{ path: "file.txt", syntax: "exact", extra: true }"#, Format::Rhai)
                .unwrap();
        let result: Result<PathPatternSpec, _> = node.as_type();
        assert!(result.is_err());
    }

    #[test]
    fn path_pattern_map_pattern_key_errors() {
        let node = Node::parse_str(r#"#{ pattern: "file.txt" }"#, Format::Rhai).unwrap();
        let result: Result<PathPatternSpec, _> = node.as_type();
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------------------------------ //
    // TextPatternSpec Parsing Tests

    #[test]
    fn text_pattern_string_exact() {
        let node = Node::parse_str("\"README.md\"", Format::Rhai).unwrap();
        let tp: TextPatternSpec = node.as_type().unwrap();
        assert_eq!(tp.pattern, "README.md");
        assert_eq!(tp.syntax, PatternSyntax::Exact);
    }

    #[test]
    fn text_pattern_string_wildcard() {
        let node = Node::parse_str("\"README*\"", Format::Rhai).unwrap();
        let tp: TextPatternSpec = node.as_type().unwrap();
        assert_eq!(tp.syntax, PatternSyntax::Wildcard);
    }

    #[test]
    fn text_pattern_explicit_map() {
        let node = Node::parse_str(r#"#{ pattern: "README*", syntax: "wildcard" }"#, Format::Rhai)
            .unwrap();
        let tp: TextPatternSpec = node.as_type().unwrap();
        assert_eq!(tp.pattern, "README*");
        assert_eq!(tp.syntax, PatternSyntax::Wildcard);
    }

    #[test]
    fn text_pattern_explicit_map_auto_detects() {
        let node = Node::parse_str(r#"#{ pattern: "README*" }"#, Format::Rhai).unwrap();
        let tp: TextPatternSpec = node.as_type().unwrap();
        assert_eq!(tp.syntax, PatternSyntax::Wildcard);
    }

    #[test]
    fn text_pattern_glob_syntax_errors() {
        let node =
            Node::parse_str(r#"#{ pattern: "[a-z]*", syntax: "glob" }"#, Format::Rhai).unwrap();
        let result: Result<TextPatternSpec, _> = node.as_type();
        assert!(result.is_err());
    }

    #[test]
    fn text_pattern_auto_detect_helper() {
        let exact = TextPatternSpec::auto_detect("README.md".to_string());
        assert_eq!(exact.syntax, PatternSyntax::Exact);

        let wildcard = TextPatternSpec::auto_detect("README*".to_string());
        assert_eq!(wildcard.syntax, PatternSyntax::Wildcard);
    }

    // ------------------------------------------------------------------------------------------ //
    // AttrRuleSpec Parsing Tests

    #[test]
    fn attr_rule_string_shorthand() {
        let node = Node::parse_str("\"rs\"", Format::Rhai).unwrap();
        let rule: AttrRuleSpec = node.as_type().unwrap();
        assert_eq!(rule.include.len(), 1);
        assert_eq!(rule.include[0].pattern, "rs");
        assert!(rule.exclude.is_empty());
    }

    #[test]
    fn attr_rule_array_shorthand() {
        let node = Node::parse_str(r#"["rs", "toml"]"#, Format::Rhai).unwrap();
        let rule: AttrRuleSpec = node.as_type().unwrap();
        assert_eq!(rule.include.len(), 2);
        assert!(rule.exclude.is_empty());
    }

    #[test]
    fn attr_rule_map_full() {
        let node =
            Node::parse_str(r#"#{ include: ["rs", "toml"], exclude: ["bak"] }"#, Format::Rhai)
                .unwrap();
        let rule: AttrRuleSpec = node.as_type().unwrap();
        assert_eq!(rule.include.len(), 2);
        assert_eq!(rule.exclude.len(), 1);
    }

    #[test]
    fn attr_rule_map_exclude_only() {
        let node = Node::parse_str(r#"#{ exclude: "bak" }"#, Format::Rhai).unwrap();
        let rule: AttrRuleSpec = node.as_type().unwrap();
        assert!(rule.include.is_empty());
        assert_eq!(rule.exclude.len(), 1);
    }

    #[test]
    fn attr_rule_empty_map() {
        let node = Node::parse_str("#{}", Format::Rhai).unwrap();
        let rule: AttrRuleSpec = node.as_type().unwrap();
        assert!(rule.include.is_empty());
        assert!(rule.exclude.is_empty());
    }

    // ------------------------------------------------------------------------------------------ //
    // SourceSpec Parsing Tests

    #[test]
    fn source_spec_string_shorthand() {
        let node = Node::parse_str("\"some/path\"", Format::Rhai).unwrap();
        let spec: SourceSpec = node.as_type().unwrap();
        let from = spec.from.unwrap();
        assert_eq!(from.root.len(), 1);
        assert_eq!(from.root[0].path, "some/path");
        assert_eq!(from.root[0].syntax, PatternSyntax::Exact);
        assert!(spec.where_.is_none());
    }

    #[test]
    fn source_spec_string_wildcard_shorthand() {
        let node = Node::parse_str("\"*.rhai\"", Format::Rhai).unwrap();
        let spec: SourceSpec = node.as_type().unwrap();
        let from = spec.from.unwrap();
        assert_eq!(from.root[0].syntax, PatternSyntax::Wildcard);
    }

    #[test]
    fn source_spec_map_from_and_where() {
        let node = Node::parse_str(
            r#"#{ from: #{ root: ["src/", "lib/"], prune: "target" }, where: #{ ext: ["rs", "toml"] } }"#,
            Format::Rhai,
        )
        .unwrap();
        let spec: SourceSpec = node.as_type().unwrap();
        let from = spec.from.unwrap();
        assert_eq!(from.root.len(), 2);
        assert_eq!(from.prune.len(), 1);
        let where_ = spec.where_.unwrap();
        assert!(where_.ext.is_some());
    }

    #[test]
    fn source_spec_where_only() {
        let node = Node::parse_str(r#"#{ where: #{ ext: "rs" } }"#, Format::Rhai).unwrap();
        let spec: SourceSpec = node.as_type().unwrap();
        assert!(spec.from.is_none());
        assert!(spec.where_.is_some());
    }

    #[test]
    fn source_spec_from_string_shorthand() {
        let node = Node::parse_str(r#"#{ from: "/some/dir" }"#, Format::Rhai).unwrap();
        let spec: SourceSpec = node.as_type().unwrap();
        let from = spec.from.unwrap();
        assert_eq!(from.root.len(), 1);
        assert_eq!(from.root[0].path, "/some/dir");
    }

    #[test]
    fn source_spec_from_array_shorthand() {
        let node = Node::parse_str(r#"#{ from: ["src", "tests"] }"#, Format::Rhai).unwrap();
        let spec: SourceSpec = node.as_type().unwrap();
        let from = spec.from.unwrap();
        assert_eq!(from.root.len(), 2);
        assert_eq!(from.root[0].path, "src");
        assert_eq!(from.root[1].path, "tests");
    }

    #[test]
    fn source_spec_from_with_omit_errors() {
        let node = Node::parse_str(
            r#"#{ from: #{ root: "src/", omit: "**/*.generated.rs" } }"#,
            Format::Rhai,
        )
        .unwrap();
        let result: Result<SourceSpec, _> = node.as_type();
        assert!(result.is_err());
    }

    #[test]
    fn source_spec_unknown_key_errors() {
        let node = Node::parse_str(r#"#{ from: "src/", unknown: true }"#, Format::Rhai).unwrap();
        let result: Result<SourceSpec, _> = node.as_type();
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------------------------------ //
    // WhereSpec Parsing Tests

    #[cfg(feature = "format-json")]
    #[test]
    fn where_spec_full() {
        let node = Node::parse_str(
            r#"{ "case": "sensitive", "path": "src/*", "name": "Cargo.toml", "stem": ["main", "lib"], "ext": { "include": ["rs"], "exclude": ["bak"] } }"#,
            Format::Json,
        )
        .unwrap();
        let w: WhereSpec = node.as_type().unwrap();
        assert_eq!(w.case, CaseMode::Sensitive);
        assert!(w.path.is_some());
        assert!(w.name.is_some());
        assert!(w.stem.is_some());
        assert!(w.ext.is_some());
    }

    #[test]
    fn where_spec_defaults() {
        let node = Node::parse_str(r#"#{ ext: "rs" }"#, Format::Rhai).unwrap();
        let w: WhereSpec = node.as_type().unwrap();
        assert_eq!(w.case, CaseMode::Insensitive);
        assert!(w.path.is_none());
        assert!(w.name.is_none());
        assert!(w.stem.is_none());
        assert!(w.ext.is_some());
    }

    // ------------------------------------------------------------------------------------------ //
    // Files Parsing Tests

    #[test]
    fn files_string_shorthand() {
        let node = Node::parse_str(r#""src/*.rs""#, Format::Rhai).unwrap();
        let f: Files = node.as_type().unwrap();
        assert_eq!(f.sources.len(), 1);
        assert_eq!(f.sources[0].from.as_ref().unwrap().root[0].syntax, PatternSyntax::Wildcard);
    }

    #[test]
    fn files_array_shorthand() {
        let node = Node::parse_str(r#"["src/*.rs", "docs/*.md"]"#, Format::Rhai).unwrap();
        let f: Files = node.as_type().unwrap();
        assert_eq!(f.sources.len(), 2);
    }

    #[test]
    fn files_single_source_map() {
        let node =
            Node::parse_str(r#"#{ from: "src", where: #{ ext: "rs" } }"#, Format::Rhai).unwrap();
        let f: Files = node.as_type().unwrap();
        assert_eq!(f.sources.len(), 1);
        assert!(f.sources[0].where_.is_some());
    }

    #[test]
    fn files_full_config() {
        let node = Node::parse_str(
            r#"#{ sources: [#{ from: "src/", where: #{ ext: ["rs"] } }, "*.rhai"] }"#,
            Format::Rhai,
        )
        .unwrap();
        let f: Files = node.as_type().unwrap();
        assert_eq!(f.sources.len(), 2);
        assert!(f.sources[0].from.is_some());
        assert!(f.sources[0].where_.is_some());
        assert!(f.sources[1].from.is_some());
        assert!(f.sources[1].where_.is_none());
    }

    #[test]
    fn files_empty_sources() {
        let node = Node::parse_str("#{ sources: [] }", Format::Rhai).unwrap();
        let f: Files = node.as_type().unwrap();
        assert!(f.sources.is_empty());
    }

    #[test]
    fn files_empty_map_is_single_implicit_source() {
        let node = Node::parse_str("#{}", Format::Rhai).unwrap();
        let files: Files = node.as_type().unwrap();
        assert_eq!(files.sources.len(), 1);
        assert!(files.sources[0].from.is_none());
    }

    #[test]
    fn files_round_trip_through_canonical_json() {
        let node = Node::parse_str(
            r#"#{ from: #{ root: #{ path: "images", recursive: false } }, where: #{ ext: ["png", "webp"] } }"#,
            Format::Rhai,
        )
        .unwrap();
        let files: Files = node.as_type().unwrap();

        let json = files.to_node().unwrap().to_string_as(Format::Json).unwrap();
        let reparsed: Files = Node::parse_str(&json, Format::Json).unwrap().as_type().unwrap();

        let source = &reparsed.sources[0];
        let root = &source.from.as_ref().unwrap().root[0];
        assert_eq!(root.path, "images");
        assert!(!root.recursive);
        assert_eq!(source.where_.as_ref().unwrap().ext.as_ref().unwrap().include.len(), 2);
    }

    // ------------------------------------------------------------------------------------------ //
    // Wildcard Escaping Tests

    #[test]
    fn escape_wildcard_for_globset_escapes_braces_and_brackets() {
        assert_eq!(escape_wildcard_for_globset("{a}[b]"), "[{]a[}][[]b[]]");
    }

    #[test]
    fn escape_wildcard_for_globset_keeps_plain_wildcards() {
        assert_eq!(escape_wildcard_for_globset("**/*.txt"), "**/*.txt");
    }

    // ------------------------------------------------------------------------------------------ //
    // Pattern Adaptation Tests

    #[test]
    fn adapt_pattern_joins_relative_with_base_dir() {
        let base = Path::new("/some/base");
        let result = adapt_pattern(Some(base), "**/*.txt").unwrap();
        assert_eq!(result, "/some/base/**/*.txt");
    }

    #[cfg(not(windows))]
    #[test]
    fn adapt_pattern_returns_absolute_unchanged() {
        let result = adapt_pattern(Some(Path::new("/base")), "/abs/**/*.rs").unwrap();
        assert_eq!(result, "/abs/**/*.rs");
    }

    #[test]
    fn adapt_pattern_errors_without_base_dir() {
        let result = adapt_pattern(None, "relative/*.txt");
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------------------------------ //
    // Extension Normalization Tests

    #[test]
    fn normalize_extension_strips_leading_dot() {
        assert_eq!(normalize_extension(".jpg"), "jpg");
    }

    #[test]
    fn normalize_extension_no_dot_unchanged() {
        assert_eq!(normalize_extension("jpg"), "jpg");
    }

    // ------------------------------------------------------------------------------------------ //
    // Test Helpers

    fn create_test_files(dir: &TempDir, names: &[&str]) {
        for name in names {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, "test content").unwrap();
        }
    }

    /// Creates a source spec with a single exact root pointing at `path`.
    fn source_with_root(path: &str) -> SourceSpec {
        SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: path.to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: None,
        }
    }

    /// Creates a source spec with root and prune.
    fn source_with_root_and_prune(root: &str, prune: &str) -> SourceSpec {
        SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: root.to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::one(PathPatternSpec {
                    path: prune.to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
            }),
            where_: None,
        }
    }

    // ------------------------------------------------------------------------------------------ //
    // SourceSpec Locate Tests

    #[test]
    fn locate_root_file() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["file.txt"]);
        let spec = source_with_root("file.txt");
        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn locate_root_missing_must_exist_errors() {
        let dir = TempDir::new().unwrap();
        let spec = source_with_root("nonexistent.txt");
        let result = spec.locate(Some(dir.path()));
        assert!(result.is_err());
    }

    #[test]
    fn locate_root_missing_not_must_exist_ok() {
        let dir = TempDir::new().unwrap();
        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "nonexistent.txt".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: false,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn locate_root_dir_recursive() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.txt", "d/sub/b.txt", "d/sub/deep/c.txt"]);
        let spec = source_with_root("d");
        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn locate_root_dir_non_recursive() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.txt", "d/b.txt", "d/sub/c.txt"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: false,
                }),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn locate_root_wildcard_expansion() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["c.txt", "a.txt", "b.txt"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "*.txt".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 3);
        // Sorted within batch.
        assert!(result[0].ends_with("a.txt"));
        assert!(result[1].ends_with("b.txt"));
        assert!(result[2].ends_with("c.txt"));
    }

    #[test]
    fn locate_root_wildcard_no_matches_ok() {
        let dir = TempDir::new().unwrap();

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "*.nonexistent".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn locate_root_wildcard_must_exist_errors() {
        let dir = TempDir::new().unwrap();

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "*.nonexistent".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path()));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("matched no files"), "error should mention no matches: {err}");
    }

    #[test]
    fn locate_root_wildcard_matching_directory() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["proj-a/x.rs", "proj-a/y.rs", "proj-b/z.rs", "other.txt"]);

        // Glob matches directories proj-a and proj-b; they should be walked.
        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "proj-*".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 3);
        let names: Vec<&str> =
            result.iter().map(|p| p.file_name().unwrap().to_str().unwrap()).collect();
        assert!(names.contains(&"x.rs"));
        assert!(names.contains(&"y.rs"));
        assert!(names.contains(&"z.rs"));
    }

    #[test]
    fn locate_root_wildcard_matching_directory_with_prune() {
        let dir = TempDir::new().unwrap();
        create_test_files(
            &dir,
            &[
                "proj-a/src/x.rs",
                "proj-a/target/debug.rs",
                "proj-b/src/y.rs",
            ],
        );

        // Use an exact prune path for the target directory. (Wildcard prune patterns like
        // `**/target` match directory paths, not the file paths inside them, so they don't
        // filter descendants via is_excluded. This is a known limitation.)
        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "proj-*".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: true,
                }),
                prune: OneOrMany::one(PathPatternSpec {
                    path: "proj-a/target".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: false,
                    recursive: true,
                }),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
        let names: Vec<&str> =
            result.iter().map(|p| p.file_name().unwrap().to_str().unwrap()).collect();
        assert!(names.contains(&"x.rs"));
        assert!(names.contains(&"y.rs"));
    }

    #[test]
    fn locate_root_wildcard_matching_directory_with_where() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["proj/a.rs", "proj/b.toml", "proj/c.txt"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "proj*".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                ext: Some(AttrRuleSpec {
                    include: OneOrMany::one(TextPatternSpec {
                        pattern: "rs".to_string(),
                        syntax: PatternSyntax::Exact,
                    }),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("a.rs"));
    }

    #[test]
    fn locate_root_wildcard_mixed_files_and_dirs() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["alpha/nested.txt", "alpha.txt"]);

        // Pattern "alpha*" matches both the file "alpha.txt" and the directory "alpha".
        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "alpha*".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
        let names: Vec<&str> =
            result.iter().map(|p| p.file_name().unwrap().to_str().unwrap()).collect();
        assert!(names.contains(&"alpha.txt"));
        assert!(names.contains(&"nested.txt"));
    }

    #[test]
    fn locate_root_wildcard_matching_directory_non_recursive() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["proj/top.rs", "proj/sub/deep.rs"]);

        // recursive=false: wildcard-matched directory should only collect direct children.
        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "proj*".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: false,
                }),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("top.rs"));
    }

    #[test]
    fn locate_preserves_root_order() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["first.mp4", "second.webm"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::new(vec![
                    PathPatternSpec {
                        path: "second.webm".to_string(),
                        syntax: PatternSyntax::Exact,
                        must_exist: true,
                        recursive: true,
                    },
                    PathPatternSpec {
                        path: "first.mp4".to_string(),
                        syntax: PatternSyntax::Exact,
                        must_exist: true,
                        recursive: true,
                    },
                ]),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result[0].ends_with("second.webm"));
        assert!(result[1].ends_with("first.mp4"));
    }

    #[test]
    fn locate_deduplication() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["file.txt"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::new(vec![
                    PathPatternSpec {
                        path: "file.txt".to_string(),
                        syntax: PatternSyntax::Exact,
                        must_exist: true,
                        recursive: true,
                    },
                    PathPatternSpec {
                        path: "file.txt".to_string(),
                        syntax: PatternSyntax::Exact,
                        must_exist: true,
                        recursive: true,
                    },
                ]),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
    }

    // ------------------------------------------------------------------------------------------ //
    // Prune Tests

    #[test]
    fn locate_prune_path_file() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/keep.txt", "d/remove.txt"]);

        let spec = source_with_root_and_prune("d", "d/remove.txt");
        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("keep.txt"));
    }

    #[test]
    fn locate_prune_dir_recursive() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.txt", "d/sub/b.txt", "d/sub/deep/c.txt"]);

        let spec = source_with_root_and_prune("d", "d/sub");
        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("a.txt"));
    }

    #[test]
    fn locate_prune_dir_non_recursive() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.txt", "d/sub/b.txt", "d/sub/deep/c.txt"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::one(PathPatternSpec {
                    path: "d/sub".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: false,
                }),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        // a.txt and sub/deep/c.txt survive; sub/b.txt is excluded.
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn locate_prune_wildcard() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/keep.rs", "d/remove.tmp"]);

        // Prune wildcard patterns match against base_dir-relative paths.
        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::one(PathPatternSpec {
                    path: "**/*.tmp".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: true,
                }),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("keep.rs"));
    }

    #[test]
    fn locate_prune_missing_not_must_exist_ok() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.txt"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::one(PathPatternSpec {
                    path: "d/nonexistent".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: false,
                    recursive: true,
                }),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn locate_prune_missing_must_exist_errors() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.txt"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::one(PathPatternSpec {
                    path: "d/nonexistent".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path()));
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------------------------------ //
    // Where Extension Filter Tests

    #[test]
    fn locate_where_ext_include() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/video.mp4", "d/video.webm", "d/image.png"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                ext: Some(AttrRuleSpec {
                    include: OneOrMany::new(vec![
                        TextPatternSpec::exact("mp4".to_string()),
                        TextPatternSpec::exact("webm".to_string()),
                    ]),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn locate_where_ext_case_insensitive() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/video.MP4", "d/audio.mp4"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                ext: Some(AttrRuleSpec {
                    include: OneOrMany::one(TextPatternSpec::exact("mp4".to_string())),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn locate_where_ext_leading_dot() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/video.mp4", "d/image.png"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                ext: Some(AttrRuleSpec {
                    include: OneOrMany::one(TextPatternSpec::exact(".mp4".to_string())),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("video.mp4"));
    }

    #[test]
    fn locate_where_ext_empty_matches_no_extension() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/with_ext.mp4", "d/no_ext"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                ext: Some(AttrRuleSpec {
                    include: OneOrMany::one(TextPatternSpec::exact("".to_string())),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("no_ext"));
    }

    #[test]
    fn locate_where_ext_exclude() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.rs", "d/b.txt", "d/c.exe"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                ext: Some(AttrRuleSpec {
                    include: OneOrMany::default(),
                    exclude: OneOrMany::one(TextPatternSpec::exact("exe".to_string())),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|p| p.extension().unwrap() != "exe"));
    }

    #[test]
    fn locate_where_ext_exclude_wins_over_include() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.rs", "d/b.txt"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                ext: Some(AttrRuleSpec {
                    include: OneOrMany::new(vec![
                        TextPatternSpec::exact("rs".to_string()),
                        TextPatternSpec::exact("txt".to_string()),
                    ]),
                    exclude: OneOrMany::one(TextPatternSpec::exact("txt".to_string())),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("a.rs"));
    }

    // ------------------------------------------------------------------------------------------ //
    // Where Name and Stem Filter Tests

    #[test]
    fn locate_where_name_include() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/foo.txt", "d/bar.txt", "d/baz.rs"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                name: Some(AttrRuleSpec {
                    include: OneOrMany::one(TextPatternSpec::exact("foo.txt".to_string())),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("foo.txt"));
    }

    #[test]
    fn locate_where_stem_include() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/readme.txt", "d/readme.md", "d/other.txt"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                stem: Some(AttrRuleSpec {
                    include: OneOrMany::one(TextPatternSpec::exact("readme".to_string())),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn locate_where_name_wildcard() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/test_foo.rs", "d/test_bar.rs", "d/main.rs"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                name: Some(AttrRuleSpec {
                    include: OneOrMany::one(TextPatternSpec {
                        pattern: "test_*".to_string(),
                        syntax: PatternSyntax::Wildcard,
                    }),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn locate_where_case_sensitive() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/video.MP4", "d/audio.mp4"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                case: CaseMode::Sensitive,
                ext: Some(AttrRuleSpec {
                    include: OneOrMany::one(TextPatternSpec::exact("mp4".to_string())),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("audio.mp4"));
    }

    // ------------------------------------------------------------------------------------------ //
    // Pattern Syntax Tests

    #[test]
    fn locate_wildcard_brackets_literal() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["file.mp3", "file.mp4", "file.mp[34]"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "*.mp[34]".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("file.mp[34]"));
    }

    #[test]
    fn locate_wildcard_character_class_is_literal() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["file.mp3", "file.mp4", "file.mp[34]"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "*.mp[34]".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("file.mp[34]"));
    }

    #[test]
    fn locate_braces_are_literal() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["a.rs", "b.toml", "c.txt", "{rs,toml}"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "*.{rs,toml}".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        // With braces literal, this should match nothing (no file ends with ".{rs,toml}").
        assert!(result.is_empty());
    }

    #[test]
    fn prune_wildcard_braces_are_literal() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["a.rs", "b.toml", "c.txt"]);

        // Prune pattern with braces should NOT expand as alternation.
        // If braces were expanded, "*.{rs,toml}" would prune a.rs and b.toml.
        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: ".".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::one(PathPatternSpec {
                    path: "*.{rs,toml}".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: true,
                }),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        // All three files should survive - the prune pattern matches nothing because braces
        // are literal (no file ends with ".{rs,toml}").
        assert_eq!(result.len(), 3);
    }

    // ------------------------------------------------------------------------------------------ //
    // Invalid Where Pattern Tests

    #[test]
    fn locate_where_invalid_ext_pattern_errors() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["a.rs"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: ".".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                ext: Some(AttrRuleSpec {
                    include: OneOrMany::one(TextPatternSpec {
                        pattern: "[invalid".to_string(),
                        syntax: PatternSyntax::Wildcard,
                    }),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn locate_where_glob_syntax_errors() {
        let node = Node::parse_str(
            r#"#{ name: #{ include: #{ pattern: "[bad", syntax: "glob" } } }"#,
            Format::Rhai,
        )
        .unwrap();
        let result: Result<WhereSpec, _> = node.as_type();
        assert!(result.is_err());
    }

    #[test]
    fn locate_where_invalid_wildcard_pattern_is_ok() {
        // Wildcard patterns escape brackets, so `[invalid` is treated as literal `[invalid`.
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["a.rs", "[invalid.txt"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: ".".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                name: Some(AttrRuleSpec {
                    include: OneOrMany::one(TextPatternSpec {
                        pattern: "[invalid*".to_string(),
                        syntax: PatternSyntax::Wildcard,
                    }),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].file_name().unwrap().to_str().unwrap().starts_with("[invalid"));
    }

    // ------------------------------------------------------------------------------------------ //
    // Base Directory Tests

    #[test]
    fn locate_relative_path_with_base_dir() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["subdir/file.txt"]);

        let spec = source_with_root("subdir/file.txt");
        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn locate_relative_pattern_with_base_dir() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["subdir/a.txt", "subdir/b.txt"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "subdir/*.txt".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn locate_no_base_dir_relative_path_errors() {
        let spec = source_with_root("relative/file.txt");

        let result = spec.locate(None::<&Path>);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("relative"), "error should mention relative path: {err}");
    }

    #[test]
    fn locate_no_base_dir_absolute_path_works() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["file.txt"]);
        let abs_path = dir.path().join("file.txt");

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: abs_path.to_string_lossy().into_owned(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(None::<&Path>).unwrap();
        assert_eq!(result.len(), 1);
    }

    // ------------------------------------------------------------------------------------------ //
    // Pruning During Walk Tests

    #[test]
    fn pruning_skips_prune_dir_during_walk() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.txt", "d/target/b.txt", "d/target/deep/c.txt"]);

        let spec = source_with_root_and_prune("d", "d/target");
        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("a.txt"));
    }

    #[test]
    fn prune_wildcard_filters_results() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.txt", "d/target/b.txt"]);

        // Prune via wildcard: files are filtered out.
        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::one(PathPatternSpec {
                    path: "d/target/*".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: true,
                }),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("a.txt"));
    }

    #[test]
    fn prune_wildcard_absolute_pattern() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/keep.rs", "d/remove.tmp"]);

        // An absolute prune wildcard should still match even when base_dir is set.
        let abs_pattern = format!("{}/**/*.tmp", dir.path().display());
        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::one(PathPatternSpec {
                    path: abs_pattern,
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: true,
                }),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("keep.rs"));
    }

    #[test]
    fn pruning_skips_root_entirely() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.txt", "d/sub/b.txt"]);

        // Root "d" is itself pruned.
        let spec = source_with_root_and_prune("d", "d");
        let result = spec.locate(Some(dir.path())).unwrap();
        assert!(result.is_empty());
    }

    // ------------------------------------------------------------------------------------------ //
    // Path Filter Tests

    /// Creates a source spec with a path exclude rule.
    fn source_with_root_and_path_exclude(root: &str, exclude: &str) -> SourceSpec {
        SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: root.to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                path: Some(AttrRuleSpec {
                    include: OneOrMany::default(),
                    exclude: OneOrMany::one(TextPatternSpec::auto_detect(exclude.to_string())),
                }),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn locate_where_path_exact_relative_include() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.txt", "d/sub/b.txt", "d/sub/c.txt"]);

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                path: Some(AttrRuleSpec {
                    include: OneOrMany::one(TextPatternSpec::exact("sub/b.txt".to_string())),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("b.txt"));
    }

    #[test]
    fn locate_where_path_wildcard_relative_exclude() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/keep.rs", "d/remove.tmp", "d/sub/also.tmp"]);

        let spec = source_with_root_and_path_exclude("d", "**/*.tmp");
        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("keep.rs"));
    }

    #[test]
    fn locate_where_path_wildcard_absolute_exclude() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/keep.rs", "d/skip.tmp"]);

        let abs_pattern = format!("{}/**/*.tmp", dir.path().display());
        let spec = source_with_root_and_path_exclude("d", &abs_pattern);
        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("keep.rs"));
    }

    #[test]
    fn locate_where_path_relative_include_matches_each_source_root() {
        let dir = TempDir::new().unwrap();
        create_test_files(
            &dir,
            &[
                "project-a/src/main.rs",
                "project-a/readme.md",
                "project-b/src/main.rs",
                "project-b/readme.md",
            ],
        );

        let source = |root: &str| SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: root.to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                path: Some(AttrRuleSpec {
                    include: OneOrMany::one(TextPatternSpec::auto_detect("src/*.rs".to_string())),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let files = Files {
            sources: vec![source("project-a"), source("project-b")],
        };
        let result = files.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|p| p.ends_with("src/main.rs")));
    }

    #[test]
    fn locate_where_path_wildcard_root_uses_each_matched_dir_as_base() {
        let dir = TempDir::new().unwrap();
        create_test_files(
            &dir,
            &[
                "2020/img/a.png",
                "2020/thumbs/a.png",
                "2021/img/b.png",
                "2021/thumbs/b.png",
            ],
        );

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "202*".to_string(),
                    syntax: PatternSyntax::Wildcard,
                    must_exist: false,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: Some(WhereSpec {
                path: Some(AttrRuleSpec {
                    include: OneOrMany::one(TextPatternSpec::auto_detect("img/*.png".to_string())),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|p| p.ends_with("a.png")));
        assert!(result.iter().any(|p| p.ends_with("b.png")));
    }

    #[test]
    fn locate_where_path_combined_with_prune() {
        let dir = TempDir::new().unwrap();
        create_test_files(
            &dir,
            &[
                "d/keep.rs",
                "d/remove.tmp",
                "d/target/pruned.rs",
                "d/target/deep/pruned.txt",
            ],
        );

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "d".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
                prune: OneOrMany::one(PathPatternSpec {
                    path: "d/target".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: true,
                    recursive: true,
                }),
            }),
            where_: Some(WhereSpec {
                path: Some(AttrRuleSpec {
                    include: OneOrMany::default(),
                    exclude: OneOrMany::one(TextPatternSpec::auto_detect("**/*.tmp".to_string())),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("keep.rs"));
    }

    // ------------------------------------------------------------------------------------------ //
    // Implicit From Tests

    #[test]
    fn locate_implicit_from_uses_base_dir() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["a.rs", "b.txt"]);

        // No from spec - should default to walking base_dir.
        let spec = SourceSpec {
            from: None,
            where_: Some(WhereSpec {
                ext: Some(AttrRuleSpec {
                    include: OneOrMany::one(TextPatternSpec::exact("rs".to_string())),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("a.rs"));
    }

    #[test]
    fn locate_implicit_from_no_base_dir_errors() {
        let spec = SourceSpec {
            from: None,
            where_: None,
        };

        let result = spec.locate(None::<&Path>);
        assert!(result.is_err());
    }

    #[test]
    fn locate_empty_from_defaults_to_base_dir() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["a.rs", "b.txt", "sub/c.rs"]);

        // from is Some but root is empty - should default to walking base_dir.
        let spec = SourceSpec {
            from: Some(FromSpec::default()),
            where_: Some(WhereSpec {
                ext: Some(AttrRuleSpec {
                    include: OneOrMany::one(TextPatternSpec::exact("rs".to_string())),
                    exclude: OneOrMany::default(),
                }),
                ..Default::default()
            }),
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
        let names: Vec<&str> =
            result.iter().map(|p| p.file_name().unwrap().to_str().unwrap()).collect();
        assert!(names.contains(&"a.rs"));
        assert!(names.contains(&"c.rs"));
    }

    // ------------------------------------------------------------------------------------------ //
    // Symlink Tests

    #[cfg(unix)]
    #[test]
    fn locate_dir_includes_symlinked_file() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/real.txt"]);

        std::os::unix::fs::symlink(dir.path().join("d/real.txt"), dir.path().join("d/link.txt"))
            .unwrap();

        let spec = source_with_root("d");

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2, "should include both real file and symlinked file");
        let names: Vec<_> = result.iter().map(|p| p.file_name().unwrap()).collect();
        assert!(names.contains(&std::ffi::OsStr::new("link.txt")));
        assert!(names.contains(&std::ffi::OsStr::new("real.txt")));
    }

    #[cfg(unix)]
    #[test]
    fn locate_dir_does_not_recurse_symlinked_dir() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.txt", "other/b.txt"]);

        std::os::unix::fs::symlink(dir.path().join("other"), dir.path().join("d/linked")).unwrap();

        let spec = source_with_root("d");

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1, "should not recurse into symlinked directory");
        assert!(result[0].ends_with("a.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn locate_dir_skips_broken_symlink() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.txt"]);

        std::os::unix::fs::symlink("/nonexistent/target", dir.path().join("d/broken.txt")).unwrap();

        let spec = source_with_root("d");

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1, "broken symlink should be silently skipped");
        assert!(result[0].ends_with("a.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn locate_root_symlink_to_dir_errors() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["real_dir/a.txt"]);

        std::os::unix::fs::symlink(dir.path().join("real_dir"), dir.path().join("link_to_dir"))
            .unwrap();

        let spec = source_with_root("link_to_dir");

        let result = spec.locate(Some(dir.path()));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("symlink"), "error should mention symlink: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn locate_root_symlink_to_file_works() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["real.txt"]);

        std::os::unix::fs::symlink(dir.path().join("real.txt"), dir.path().join("link.txt"))
            .unwrap();

        let spec = source_with_root("link.txt");

        let result = spec.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("link.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn locate_root_broken_symlink_must_exist_errors() {
        let dir = TempDir::new().unwrap();

        std::os::unix::fs::symlink("/nonexistent/target", dir.path().join("broken")).unwrap();

        let spec = source_with_root("broken");

        let result = spec.locate(Some(dir.path()));
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn locate_root_broken_symlink_not_must_exist_ok() {
        let dir = TempDir::new().unwrap();

        std::os::unix::fs::symlink("/nonexistent/target", dir.path().join("broken")).unwrap();

        let spec = SourceSpec {
            from: Some(FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: "broken".to_string(),
                    syntax: PatternSyntax::Exact,
                    must_exist: false,
                    recursive: true,
                }),
                prune: OneOrMany::default(),
            }),
            where_: None,
        };

        let result = spec.locate(Some(dir.path())).unwrap();
        assert!(result.is_empty());
    }

    // ------------------------------------------------------------------------------------------ //
    // Files (Multi-Source) Tests

    #[test]
    fn files_locate_empty_sources() {
        let files = Files::default();
        let result = files.locate(Some(Path::new("/tmp"))).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn files_locate_single_source() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.txt", "d/b.txt"]);

        let files = Files {
            sources: vec![source_with_root("d")],
        };

        let result = files.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn files_locate_multiple_sources() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["src/lib.rs", "src/main.rs", "docs/readme.md"]);

        let files = Files {
            sources: vec![
                SourceSpec {
                    from: Some(FromSpec {
                        root: OneOrMany::one(PathPatternSpec {
                            path: "src".to_string(),
                            syntax: PatternSyntax::Exact,
                            must_exist: true,
                            recursive: true,
                        }),
                        prune: OneOrMany::default(),
                    }),
                    where_: Some(WhereSpec {
                        ext: Some(AttrRuleSpec {
                            include: OneOrMany::one(TextPatternSpec::exact("rs".to_string())),
                            exclude: OneOrMany::default(),
                        }),
                        ..Default::default()
                    }),
                },
                SourceSpec {
                    from: Some(FromSpec {
                        root: OneOrMany::one(PathPatternSpec {
                            path: "docs".to_string(),
                            syntax: PatternSyntax::Exact,
                            must_exist: true,
                            recursive: true,
                        }),
                        prune: OneOrMany::default(),
                    }),
                    where_: Some(WhereSpec {
                        ext: Some(AttrRuleSpec {
                            include: OneOrMany::one(TextPatternSpec::exact("md".to_string())),
                            exclude: OneOrMany::default(),
                        }),
                        ..Default::default()
                    }),
                },
            ],
        };

        let result = files.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn files_locate_deduplication_across_sources() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["d/a.txt", "d/b.txt"]);

        let files = Files {
            sources: vec![source_with_root("d"), source_with_root("d")],
        };

        let result = files.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn files_locate_order_across_sources() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir, &["first/a.txt", "second/b.txt"]);

        let files = Files {
            sources: vec![source_with_root("first"), source_with_root("second")],
        };

        let result = files.locate(Some(dir.path())).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result[0].ends_with("a.txt"));
        assert!(result[1].ends_with("b.txt"));
    }
}
