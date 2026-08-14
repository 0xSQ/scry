//! Pattern-based file selection.
//!
//! The [`Files`] utility describes file selection and ordering with three controls:
//!
//! 1. **`from`** - Discovery rules: which roots to walk and which subtrees to prune.
//! 2. **`where`** - Filtering rules: which discovered files to keep.
//! 3. **`sort`** - Ordering rules for each discovered root batch.
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
//!
//! # Unicode path behavior
//!
//! [`Files`] only processes and returns paths with an exact Unicode representation. Used bases,
//! resolved exact roots, and resolved exact prune paths must be Unicode and fail structurally if
//! they are not. Non-Unicode wildcard matches and directory entries encountered during discovery
//! follow [`OnError`]. No files below a rejected directory enter results, and Scry's own directory
//! walker does not recurse into it.

use crate::desc::{self, Desc};
use crate::kit::OneOrMany;
use crate::node::{Node, NodeError};
use crate::traits::{Describe, FromNode};
use crate::util::PathSort;
use crate::ToNode;
use globset::{GlobBuilder, GlobMatcher, GlobSet, GlobSetBuilder};
use indexmap::IndexSet;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------------- //

/// Pattern-based file selection specification.
///
/// Holds one or more [`SourceSpec`]s. Each source defines discovery rules (`from`), filtering
/// rules (`where`), and root-batch ordering (`sort`). Results are merged across sources with
/// deduplication.
#[derive(Debug, Clone, Default, ToNode)]
pub struct Files {
    /// The sources to collect files from.
    pub sources: Vec<SourceSpec>,
}

/// Specifies a collection of files via discovery, filtering, and ordering rules.
///
/// Can be specified as a string (shorthand for a single `from.root` entry) or a map with
/// optional `from`, `where`, and `sort` fields. If `from` is omitted, the caller-provided
/// `base_dir` is used as the implicit root. Natural sorting is used unless `sort` selects
/// lexicographic ordering.
#[derive(Debug, Clone, Default, ToNode)]
pub struct SourceSpec {
    /// Discovery rules: where to walk and what to skip.
    pub from: Option<FromSpec>,
    /// Filtering rules: which discovered files to keep.
    #[scry(rename = "where")]
    pub where_: Option<WhereSpec>,
    /// The ordering applied to each discovered root batch.
    pub sort: PathSort,
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
    /// A path encountered during resolution cannot be represented as Unicode.
    #[error("path cannot be represented as Unicode: {path:?}")]
    NonUnicodePath { path: PathBuf },
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

/// Controls how recoverable discovery errors are handled.
///
/// This affects errors encountered while walking directories (`read_dir`, reading entries,
/// `file_type`), iterating wildcard matches, and rejecting non-Unicode entries discovered under
/// an otherwise usable root. Structural errors always fail regardless. These include non-Unicode
/// used bases and resolved exact roots or prune paths, invalid patterns, missing required paths,
/// and unsupported path types.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OnError {
    /// Stops on the first recoverable discovery error.
    Fail,
    /// Logs a warning to stderr and continues.
    #[default]
    Warn,
    /// Silently skips the problematic entry and continues.
    Ignore,
}

impl OnError {
    /// Handles a recoverable discovery error according to this policy.
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
                sort: PathSort::Natural,
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
            let sort: PathSort = node.opt("sort")?.unwrap_or_default();

            node.ensure_no_unknown_keys()?;
            return Ok(SourceSpec { from, where_, sort });
        }

        Err(NodeError::invalid_value(
            &node.path,
            "expected string or map with optional 'from'/'where'/'sort' keys",
        ))
    }
}

impl Describe for SourceSpec {
    fn describe() -> Desc {
        Desc::plain("one source: where to search and which files to keep")
    }
}

impl ToNode for PathSort {
    fn to_node(&self) -> Result<Node, NodeError> {
        match self {
            PathSort::Natural => "natural",
            PathSort::Lexicographic => "lexicographic",
        }
        .to_node()
    }
}

impl FromNode for PathSort {
    fn from_node(node: &Node) -> Result<Self, NodeError> {
        let s: String = node.as_type()?;
        match s.as_str() {
            "natural" => Ok(PathSort::Natural),
            "lexicographic" => Ok(PathSort::Lexicographic),
            _ => Err(NodeError::invalid_value(
                &node.path,
                "expected \"natural\" or \"lexicographic\"",
            )),
        }
    }
}

impl Describe for PathSort {
    fn describe() -> Desc {
        Desc::plain("path ordering mode")
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
    /// across sources while preserving first-seen order. Recoverable discovery errors use
    /// [`OnError::Warn`].
    pub fn locate(&self, base_dir: Option<&Path>) -> Result<Vec<PathBuf>, FilesError> {
        self.locate_with(base_dir, OnError::default())
    }

    /// Collects file paths from all sources, with explicit error handling policy.
    ///
    /// Like [`locate`](Self::locate), but lets the caller choose how recoverable directory,
    /// wildcard, and non-Unicode entry errors are handled.
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
    /// as the implicit root. Used bases and resolved exact roots must be Unicode. Recoverable
    /// discovery errors use [`OnError::Warn`].
    pub fn locate(&self, base_dir: Option<&Path>) -> Result<Vec<PathBuf>, FilesError> {
        self.locate_with(base_dir, OnError::default())
    }

    /// Collects file paths matching this source with an explicit recoverable-error policy.
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
                require_unicode_path(base)?;
                default_from = FromSpec {
                    root: OneOrMany::one(PathPatternSpec {
                        path: String::new(),
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
            require_unicode_path(base)?;
            default_root_from = FromSpec {
                root: OneOrMany::one(PathPatternSpec {
                    path: String::new(),
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
                self.sort,
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
    /// Whether relative wildcard matching uses the caller-provided base directory.
    has_relative_wildcard: bool,
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
        let mut has_relative_wildcard = false;

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
                    if !Path::new(&pp.path).is_absolute() {
                        has_relative_wildcard = true;
                        if let Some(base_dir) = base_dir {
                            require_unicode_path(base_dir)?;
                        }
                    }
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
            has_relative_wildcard,
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
            if self.has_relative_wildcard {
                let Some(bd) = base_dir else {
                    return false;
                };
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
    sort: PathSort,
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
                        sort.sort(&mut batch);
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
            sort.sort(&mut batch);
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
        if let Err(error) = require_unicode_path(&path) {
            on_error.handle(error)?;
            continue;
        }
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
        let path = entry.path();
        if let Err(error) = require_unicode_path(&path) {
            on_error.handle(error)?;
            continue;
        }
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                on_error.handle(FilesError::DirTraversalIo { path, source: e })?;
                continue;
            }
        };

        if ft.is_file() {
            if where_filter.is_none_or(|f| f.matches(&path, source_root)) {
                batch.push(path);
            }
        } else if recursive && ft.is_dir() {
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
            let target_meta = match std::fs::metadata(&path) {
                Ok(meta) => meta,
                Err(_) => continue, // Broken symlink: skip silently.
            };
            if target_meta.is_file() && where_filter.is_none_or(|f| f.matches(&path, source_root)) {
                batch.push(path);
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
        debug_assert!(path.to_str().is_some());
        debug_assert!(source_root.to_str().is_some());
        if let Some(ref rule) = self.path {
            if !rule.matches(path, source_root) {
                return false;
            }
        }

        // Check name filter.
        if let Some(ref rule) = self.name {
            let name = match path.file_name() {
                Some(name) => name.to_str().expect("validated path has a Unicode file name"),
                None => return false,
            };
            if !rule.matches(name) {
                return false;
            }
        }

        // Check stem filter.
        if let Some(ref rule) = self.stem {
            let stem = match path.file_stem() {
                Some(stem) => stem.to_str().expect("validated path has a Unicode file stem"),
                None => return false,
            };
            if !rule.matches(stem) {
                return false;
            }
        }

        // Check ext filter.
        if let Some(ref rule) = self.ext {
            // Extension can be None (file without extension). Represent as "".
            let ext = path
                .extension()
                .map(|ext| ext.to_str().expect("validated path has a Unicode extension"))
                .unwrap_or("");
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
        require_unicode_path(path)?;
        return Ok(path.to_path_buf());
    }
    match base_dir {
        Some(dir) => {
            require_unicode_path(dir)?;
            let resolved = dir.join(path);
            require_unicode_path(&resolved)?;
            Ok(resolved)
        }
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
            let mut base = normalize_path_separators(require_unicode_path(dir)?);
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

/// Requires a path to have an exact Unicode representation.
fn require_unicode_path(path: &Path) -> Result<&str, FilesError> {
    path.to_str().ok_or_else(|| FilesError::NonUnicodePath {
        path: path.to_path_buf(),
    })
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
    Some(normalize_path_separators(
        candidate.to_str().expect("validated candidate path is Unicode"),
    ))
}

/// Normalizes an extension string by removing a leading dot.
fn normalize_extension(ext: &str) -> &str {
    ext.strip_prefix('.').unwrap_or(ext)
}

// ---------------------------------------------------------------------------------------------- //

#[cfg(test)]
mod tests;
