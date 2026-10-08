# SeqExpr

SeqExpr gives command-line tools a compact syntax for sequences of numbers and selections from
a list. Say you're writing a tool that processes an ordered list of files. To try it on every
other file from the first twenty, a user could list the indices individually:

```console
process --select "0,2,4,6,8,10,12,14,16,18"
```

With a sequence expression, that becomes:

```console
process --select "0..20:2"
```

`0..20` starts at 0 and stops before 20. `:2` advances in steps of two. The expression stays
short even when the user wants hundreds of positions, and it describes the pattern directly.

The `process` command is an example of how your application might expose SeqExpr. The utility
lives in `scry::kit::seq_expr`. Your tool chooses the flag, orders the collection, and supplies
its length when evaluating a selection. Quote expressions on the command line so brackets and
spaces reach the program unchanged.

## When the selection grows

Often you do not know the final index, or you want something less regular than one range.
The same argument can describe an open range, a suffix, or several selections joined together:

```console
process --select "20.."
process --select "N-6.."
process --select "0,[4..12]:2,N-1"
```

`20..` means index 20 onward. `N-6..` means the final six items, with `N` supplied by the
collection's length. The last expression selects the first item, every second index from 4
through 12, and the final item. A user can extend a selection without needing a new combination
of command-line options for each case.

Order and duplicates are preserved. `2,0,2` selects the third item, then the first, then the
third again. Reversing a range's endpoints makes it run backward. Index selection checks that
every generated position exists, so asking for the final six items fails on a shorter collection.

## Floating-point sequences

The same language can generate floating-point values, for example when trying several settings
for a threshold or a strength:

```text
[0.0..0.3]:0.1   -> 0.0, 0.1, 0.2, 0.3
[0.0..1.0]/4     -> 0.0, 0.25, 0.5, 0.75, 1.0
```

The first expression supplies a step. The second divides the interval into four equal parts,
giving five values when both endpoints are included. Integer sequences support subdivision too,
with each position rounded to an integer.

The sections below describe the full language, its sampling rules, and the Rust API.

## Grammar

| Form | Meaning |
| --- | --- |
| `7` | One value. |
| `a..b` | Include the start and exclude the stop. Equivalent to `[a..b)`. |
| `[a..b]` | Include both endpoints. |
| `[a..b)` | Include only the start. |
| `(a..b]` | Include only the stop. |
| `(a..b)` | Exclude both endpoints. |
| `range:step` | Advance from the authored start by a positive step magnitude. |
| `range/count` | Divide the interval into that many equal subintervals. |
| `term,term` | Concatenate terms in authored order, retaining duplicates. |
| `a..`, `..b`, `..` | Obtain omitted integer endpoints from evaluation context. |
| `N`, `N-k`, `N+k` | Finite source length, optionally offset by an unsigned integer. |

Numbers accept signs, integer spelling, decimal fractions, and exponents. Examples are `-12`,
`+12`, `0.5`, `1.0`, `1e-3`, and `-1.25E+2`. A decimal point requires a digit on both sides.
Spellings such as `.5`, `1.`, `1_000`, hexadecimal values, type suffixes, `NaN`, and `inf` are
invalid. Counts use unsigned decimal digits and must be positive.

Whitespace can separate tokens, but cannot split a number or `..`. A bracketed range requires
both delimiters. Singletons cannot have delimiters or samplers. Empty expressions, empty comma
terms, trailing commas, nonpositive steps, and multiple samplers are errors.

Integer sequences reject decimal or exponent spelling, including `3.0` and `3e0`. Integer ranges
default to a unit step. Real sequences accept integer, decimal, and exponent literals. Every real
range needs an explicit `:step` or `/count`, including a range with equal endpoints. Real
sequences reject omitted endpoints and symbolic values.

The language has no general arithmetic. In particular, parentheses mean endpoint exclusion,
not grouping. `0.0..10.0/2` is a valid shorthand for `[0.0..10.0)/2`.

## Samplers and rounding

The resolved endpoint order determines direction. Step magnitudes remain positive for descending
ranges:

```text
3..10:2   -> 3, 5, 7, 9
10..3:2   -> 10, 8, 6, 4
```

Numeric literals, endpoint comparisons, steps, anchors, and cardinalities use exact arithmetic.
Only retained values must fit the output type. Empty ranges have no values to convert, but
required context and output limits still apply.

### Fixed steps

A fixed-step lattice is anchored at the authored start. Excluding the start advances one complete
step. An included stop appears only when the lattice lands on it:

```text
(0..6]:2    -> 2, 4, 6
[0..5]:2    -> 0, 2, 4
[0..1]:0.3  -> 0, 0.3, 0.6, 0.9
[1..0]:0.3  -> 1, 0.7, 0.4, 0.1
```

Reversing endpoints need not reverse a fixed-step result because it changes the lattice anchor.
Equal endpoints produce one value only when both are included. Otherwise the result is empty.

### Subdivision

`/n` creates `n` equal subintervals and `n + 1` anchors before endpoint exclusion:

| Endpoint inclusion | Retained anchors |
| --- | ---: |
| Both included | `n + 1` |
| One included | `n` |
| Both excluded | `n - 1` |

```text
[0.0..10.0)/2  -> 0.0, 5.0
[0.0..10.0]/2  -> 0.0, 5.0, 10.0
[5..5]/3       -> 5, 5, 5, 5
(0..1)/1       -> empty
```

Integer subdivision rounds each complete exact anchor to the nearest integer, with halfway cases
away from zero. Inclusion rules select anchors before rounding:

```text
[0..10]/4  -> 0, 3, 5, 8, 10
[-5..5]/4  -> -5, -3, 0, 3, 5
[0..2]/4   -> 0, 1, 1, 2, 2
[0..1)/2   -> 0, 1
```

The last result contains 1 because the retained interior anchor 0.5 rounds to 1. Oversampling,
duplicate values, and aliases of excluded endpoints are valid numeric results.

Large endpoints can produce small retained values. For example,
`(-9223372036854775809..9223372036854775809)/2` emits only `0`, which fits `i64`.

### Exact real anchors

`[0..0.3]:0.1` produces four exact anchors, including the stop. Repeated floating addition does
not determine whether the last anchor is retained.

Each retained real anchor is rounded once to the requested `f32` or `f64` using nearest rounding
with ties to even. `f32` evaluation converts the exact anchor directly to `f32`.
Results are finite and monotonic in the range's direction. Floating zeros are normalized to
positive zero. Underflow, duplicate floats, and aliases of excluded endpoints are ordinary
rounding outcomes. For example, `[0..1e-1000]/2` produces three zeros.

An emitted value that cannot become finite in the requested type is an evaluation error.

## Rust API and numeric profiles

Expression types check syntax and the selected numeric profile. Evaluation checks context and
output representability later. A literal outside the output type's range or an unresolved `N`
can therefore parse successfully:

| Type | Validation while parsing | Evaluation |
| --- | --- | --- |
| `SeqExpr` | Grammar and parser limits. | Convert to an integer or real profile first. |
| `IntSeqExpr` | Grammar, limits, and integer-spelled literals and steps. | `IntEvaluator` or `IndexEvaluator`. |
| `RealSeqExpr` | Grammar and limits. Ranges require both endpoints and explicit sampling. | `RealEvaluator`. |

All three types implement `FromStr`. `SeqExpr::parse_with_limits` accepts explicit parser limits.
The profile types also implement `TryFrom<SeqExpr>` and expose that expression through `as_expr()`.

`IntEvaluator::evaluate` returns `Vec<i64>`:

```rust
use scry::kit::seq_expr::{IntEvalOptions, IntEvaluator, IntSeqExpr};

let expression: IntSeqExpr = "[0..10]/4".parse()?;
let values = IntEvaluator::new(IntEvalOptions::default()).evaluate(&expression)?;
assert_eq!(values, vec![0, 3, 5, 8, 10]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`RealEvaluator` returns `Vec<f64>`, or `Vec<f32>` through `evaluate_f32`:

```rust
use scry::kit::seq_expr::{RealEvalOptions, RealEvaluator, RealSeqExpr};

let expression: RealSeqExpr = "[0..0.3]:0.1".parse()?;
let evaluator = RealEvaluator::new(RealEvalOptions::default());
assert_eq!(evaluator.evaluate(&expression)?, vec![0.0, 0.1, 0.2, 0.3]);
assert_eq!(evaluator.evaluate_f32(&expression)?, vec![0.0_f32, 0.1, 0.2, 0.3]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`IndexEvaluator` supplies the collection length and returns checked `usize` positions:

```rust
use scry::kit::seq_expr::{IndexEvaluator, IntSeqExpr};

let selection: IntSeqExpr = "N-3..".parse()?;
let indices = IndexEvaluator::default().evaluate(&selection, 12)?;
assert_eq!(indices, vec![9, 10, 11]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`source()` and `Display` return the stored expression. Parsing preserves whitespace and numeric
spelling exactly:

```rust
use scry::kit::seq_expr::{IntSeqExpr, SeqExpr};

let authored = "  +1, [2..6]:2  ";
let parsed: SeqExpr = authored.parse()?;
let integers = IntSeqExpr::try_from(parsed)?;
assert_eq!(integers.source(), authored);
assert_eq!(integers.to_string(), authored);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`IntSeqExpr::single(value)` and `IntSeqExpr::open_range()` are infallible. `single` accepts Scry's
ten native integer types, including `u64`, `isize`, and `usize`, and stores canonical decimal text.
`open_range` stores `".."` and requires evaluation context to supply its endpoints:

```rust
use scry::kit::seq_expr::{IndexEvaluator, IntSeqExpr};

let one = IntSeqExpr::single(u64::MAX);
assert_eq!(one.source(), "18446744073709551615");

let all = IntSeqExpr::open_range();
assert_eq!(all.source(), "..");
assert_eq!(IndexEvaluator::default().evaluate(&all, 3)?, vec![0, 1, 2]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Scry configuration

Choose a field type according to when you need the values and whether you need the expression's
source:

| Field | Input | Evaluation | `ToNode` output |
| --- | --- | --- | --- |
| Plain numeric `Vec<T>` | Array. | Primitive conversion during decoding. | Numeric array. |
| `SeqExpr`, `IntSeqExpr`, `RealSeqExpr` | Expression string. | Later, through an evaluator. | Stored expression string. |
| `Vec<T>` with an `int_sequence` or `real_sequence` adapter | Expression string or array. | During decoding. | Numeric array. |

Numeric arrays use Scry's primitive conversions, including numeric strings such as
`"20"`. Their scalar entries never expand sequence terms, so `["3..7"]` is invalid. Integer arrays
reject floating-point values. A whole expression string requires an expression type or sequence
adapter rather than a plain `Vec`.

### Retaining expressions

An experiment can store concrete iteration values alongside a positional selector and a real
sweep to evaluate later:

```rust
use scry::kit::seq_expr::{IndexEvaluator, IntSeqExpr, RealSeqExpr};
use scry::node::Format;
use scry::{Config, Node, ToNode};

#[derive(Config, ToNode)]
struct Experiment {
    iterations: Vec<i64>,
    #[scry(default = IntSeqExpr::open_range())]
    select: IntSeqExpr,
    comparison: Option<IntSeqExpr>,
    strengths: RealSeqExpr,
}

let input =
    r#"#{ iterations: [10, "20", 30], select: " N-2.. ", strengths: "[0..0.3]:0.1" }"#;
let config: Experiment = Node::parse_str(input, Format::Rhai)?.as_type()?;

let indices = IndexEvaluator::default().evaluate(&config.select, config.iterations.len())?;
assert_eq!(indices, vec![1, 2]);

let output = config.to_node()?;
assert_eq!(output.req::<String>("select")?, " N-2.. ");
assert_eq!(output.req::<String>("strengths")?, "[0..0.3]:0.1");
assert_eq!(output.req::<Option<String>>("comparison")?, None);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Decoding checks the expression strings and their profiles without expanding them. Apply
configuration overrides before decoding, then supply the final collection length when evaluating
the selector. The same expression can be reused against different lengths. Serialization preserves
the spaces around `N-2..` and writes the absent comparison as null.

Missing and null values follow the field's declared type and fallback:

- `iterations` and `strengths` are required. Missing either key is an error.
- Missing `select` uses `IntSeqExpr::open_range()`, whose source is `".."`. A present null is
  still invalid.
- Missing or null `comparison` becomes `None`. A supplied expression string becomes `Some`.

Expression types have no Rust `Default` or Scry `FromDefaults`. A field's explicit default
constructs its target value directly. This also applies to adapted vectors, whose defaults are
ordinary vectors rather than expression inputs. A direct expression or sequence adapter rejects
null. Use an outer `Option` when null should mean absence.

The examples use Rhai, which needs no format feature flag. Other Scry formats use the same
arrays and expression strings. JSON requires `format-json`.

### Expanding expressions into vectors

The `int_sequence` and `real_sequence` modules provide whole-vector adapters for each supported
numeric type. Select the module matching the field's element type. An expression is evaluated
directly into that type:

```rust
use scry::kit::seq_expr::{int_sequence, real_sequence};
use scry::node::Format;
use scry::{Config, Node, ToNode};

#[derive(Config, ToNode)]
struct Sweep {
    #[scry(with(int_sequence::u32))]
    iterations: Vec<u32>,
    #[scry(with(real_sequence::f32))]
    strengths: Vec<f32>,
}

for input in [
    r#"#{ iterations: "[10..30]:10", strengths: "[0..0.3]:0.1" }"#,
    r#"#{ iterations: [10,20,30], strengths: [0.0,0.1,0.2,0.3] }"#,
] {
    let config: Sweep = Node::parse_str(input, Format::Rhai)?.as_type()?;
    assert_eq!(config.iterations, vec![10, 20, 30]);
    assert_eq!(config.strengths, vec![0.0, 0.1, 0.2, 0.3]);

    let output = config.to_node()?;
    assert_eq!(output.req::<Vec<u32>>("iterations")?, vec![10, 20, 30]);
    assert_eq!(output.req::<Vec<f32>>("strengths")?, vec![0.0, 0.1, 0.2, 0.3]);
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Both inputs produce the same vectors. They retain the values rather than the source or input form.

| Adapter namespace | Concrete modules |
| --- | --- |
| `int_sequence` | `i8`, `i16`, `i32`, `i64`, `isize`, `u8`, `u16`, `u32`, `u64`, `usize`. |
| `real_sequence` | `f32`, `f64`. |

Each concrete module exports `from_node`, `to_node`, and `describe`. The functions are also
usable directly. Output borrows a slice, so vectors and arrays can be serialized without an
intermediate representation collection.

The full target range is available. For a `Vec<u64>`,
`"18446744073709551615..18446744073709551616"` emits `[u64::MAX]` with an excluded stop beyond
that type's maximum. Negative retained values fail for unsigned fields. Range errors identify
the target type, and `isize` and `usize` use the current platform's width.

Immediate expansion supplies no integer context. `"N-1"` and `".."` fail during decoding.
Adapters do not infer context from neighboring fields or arrays. Retain an `IntSeqExpr` when
evaluation needs a length or numeric bounds supplied later.

Real arrays convert numbers as supplied by the format reader. Expression strings keep exact
decimal anchors until rounding to the field's type. Empty arrays are valid. Empty strings are
invalid, while a valid expression that emits no values produces an empty vector.

Real sequence adapters require every decoded or serialized value to be finite, including values
supplied as native arrays. Native array entries such as `"NaN"` or `"inf"` are rejected.
Signed zero from array entries is preserved. Expression-derived zero is normalized to positive
zero. Array conversion and finiteness failures identify the offending element.

### Optional and grouped sequences

A selected module handles the complete field value. Compose the standard helpers in a small
module when the field wraps a sequence in `Option` or groups several sequences in an outer
vector. The inner sequence adapter owns each complete numeric vector:

```rust
use scry::node::Format;
use scry::{Config, Node, ToNode};

mod optional_strengths {
    use scry::convert::{read, write};
    use scry::kit::seq_expr::real_sequence;
    use scry::{Desc, Node, NodeError};

    pub fn from_node(node: &Node) -> Result<Option<Vec<f64>>, NodeError> {
        read::option(node, real_sequence::f64::from_node)
    }

    pub fn to_node(values: &Option<Vec<f64>>) -> Result<Node, NodeError> {
        write::option(values, |values| real_sequence::f64::to_node(values))
    }

    pub fn describe() -> Desc {
        real_sequence::f64::describe().nullable()
    }
}

mod grouped_iterations {
    use scry::convert::{read, write};
    use scry::kit::seq_expr::int_sequence;
    use scry::{Desc, Node, NodeError};

    pub fn from_node(node: &Node) -> Result<Vec<Vec<i64>>, NodeError> {
        read::vec(node, int_sequence::i64::from_node)
    }

    pub fn to_node(values: &[Vec<i64>]) -> Result<Node, NodeError> {
        write::list(values, |values| int_sequence::i64::to_node(values))
    }

    pub fn describe() -> Desc {
        Desc::list(int_sequence::i64::describe())
    }
}

#[derive(Config, ToNode)]
struct Groups {
    #[scry(with(optional_strengths))]
    strengths: Option<Vec<f64>>,
    #[scry(with(grouped_iterations))]
    iterations: Vec<Vec<i64>>,
}

let input = r#"#{ strengths: (), iterations: ["1..3",[7,8]] }"#;
let config: Groups = Node::parse_str(input, Format::Rhai)?.as_type()?;
assert_eq!(config.strengths, None);
assert_eq!(config.iterations, vec![vec![1, 2], vec![7, 8]]);

let output = config.to_node()?;
assert_eq!(output.req::<Option<Vec<f64>>>("strengths")?, None);
assert_eq!(output.req::<Vec<Vec<i64>>>("iterations")?, vec![vec![1, 2], vec![7, 8]]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Omitted or null `strengths` becomes `None`, and an array or expression becomes `Some`.
Each group independently accepts either input form. The sequence ceiling applies per inner
vector. The outer list follows ordinary vector-reader rules.

### Descriptions and CLI queries

Native descriptions use `sequence expression string`, `integer sequence expression string`,
or `real sequence expression string`. Adapter descriptions add the array alternative:
`integer sequence expression string or integer array` and
`real sequence expression string or real array`. Descriptions do not evaluate expressions.

Scry's CLI `--get` reads the original Node before typed conversion. An expression input therefore
remains a string in queries, and child-index queries work only for original arrays. `ToNode`
serializes the decoded fields using the representations in the table above.

## Explicit integer contexts

`IntEvalOptions::context` controls omitted endpoints and symbolic values:

| Context | Missing start / stop | `N` | Enforces an index domain |
| --- | --- | --- | --- |
| `None` | Evaluation error. | Evaluation error. | No. |
| `IntContext::Bounds { open_start, open_stop }` | The supplied numeric endpoints. | Evaluation error. | No. |
| `IntContext::FiniteSource { length }` | `0` / `length`. | `length`. | No. |
| `IndexEvaluator::evaluate(expression, length)` | `0` / `length`. | `length`. | Yes, for every emitted index. |

`Bounds` supplies defaults for omitted endpoints. It does not restrict explicit coordinates or
define a source length. It is suitable for numeric labels that need not be contiguous:

```rust
use scry::kit::seq_expr::{IntContext, IntEvalOptions, IntEvaluator, IntSeqExpr};

let expression: IntSeqExpr = "..:10".parse()?;
let evaluator = IntEvaluator::new(IntEvalOptions {
    context: Some(IntContext::Bounds {
        open_start: 10,
        open_stop: 31,
    }),
    ..IntEvalOptions::default()
});
assert_eq!(evaluator.evaluate(&expression)?, vec![10, 20, 30]);
assert_eq!(evaluator.evaluate(&"45".parse()?)?, vec![45]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`FiniteSource` resolves source-relative coordinates but still performs numeric evaluation.
Evaluating `N` with a length of 10 returns `[10]`. Use `IndexEvaluator` when that coordinate
must identify an existing element.

Numeric finite-source lengths use the full `usize` range. `IndexEvaluator` takes a `usize`
length that must also fit `i64`, even when the result is empty.

### End-relative values

Uppercase `N` is the source length and the exclusive upper index boundary. The final valid index
is `N-1`. Optional `+` or `-` offsets take an unsigned ASCII decimal magnitude. Whitespace,
zero offsets, and leading zeros are allowed, including `N - 6`, `N+0`, and `N-0006`.

Use symbolic values in integer singletons or endpoints. Steps and counts remain numeric.
`0..N:2` is valid. `0..N:N-1` and `0..N/N` are invalid.

Lowercase `n`, `+N`, `-N`, `N--1`, `N+-1`, missing magnitudes, decimal or exponent offsets,
digit separators, chained offsets, multiplication, and grouped arithmetic are invalid.

Offsets resolve exactly even when the written magnitude exceeds the output type's range.
For `IntEvaluator`, `N+1` at length `i64::MAX` is an `i64` range error, while the empty range
`N+1..N+1` succeeds.

Direction follows the resolved endpoints. `N-6..0` descends at length 10 and ascends at length 4.
Resolve required context before deciding that a term is empty. Even `N..N` needs finite-source
context.

### Strict indices

`IndexEvaluator` validates every emitted integer against `0 <= index < length`, including
rounded subdivision results. It does not clip negative coordinates, unavailable suffixes,
or oversized ranges.

| Expression at length 12 | Result |
| --- | --- |
| `..` | Indices 0 through 11. |
| `(N..0]` | Indices 11 through 0, descending. |
| `..100` | Error when an emitted index reaches 12. |
| `100..` | Error at the first emitted index, 100. |
| `[0..]` | Error because the included contextual stop is 12. |
| `-1` or `N` | Bounds error. |
| `N-13..` | Error at -1. |
| `0..100:100` | `[0]`. The unvisited boundary does not invalidate it. |
| `N-1..N+1:2` | `[11]`. |

At length 10, `[N-1..N)/2` retains exact anchors 9 and 9.5, then fails because 9.5 rounds
to index 10. To sample a nonempty source including its final valid position, use `[0..N-1]/4`.

Empty results are allowed. At length zero, `..` and `N..N` produce no indices. `N..N/2` retains
repeated anchors and fails at index zero.

## Limits

| Resource | Hard maximum |
| --- | ---: |
| Expression UTF-8 bytes | 4 MiB, or 4,194,304 bytes. |
| Decimal digits in one numeric, count, or offset token | 1,024. |
| Absolute written exponent | 4,096. |
| Values emitted by one evaluation | 1,000,000, exposed as `MAX_VALUES`. |

`ParseLimits`, `IntEvalOptions::max_values`, `RealEvalOptions::max_values`, and
`IndexEvaluator::new(max_values)` can tighten the relevant ceilings. Larger requested limits
are capped. The input byte limit bounds comma-separated terms without a separate term limit.
Literal digit limits count leading zeros, significand digits, and exponent digits. Offset
magnitudes have no exponent syntax.

Sequence adapters use the default limits. The value ceiling applies to both input forms
and to serialization, including vectors constructed directly in Rust. Parser limits apply only
to expression strings. Array length is checked before constructing the result, after
the format reader has already built the Node array.

Output limits apply cumulatively to the expression and count duplicates. Required coordinates
are resolved and each term's exact retained cardinality is checked before expanding it.
Endpoint exclusion affects that count: `(0..1)/1000001` retains one million anchors, while
`[0..1]/1000000` exceeds the ceiling.

These are numeric-core limits. Applications can impose additional constraints on selected
values or products of independently evaluated sequences.

## Typed diagnostics and streaming

| Error type | Failure stage |
| --- | --- |
| `ParseError` | Grammar or parser resource limits. |
| `ProfileError` | Integer or real profile validation. |
| `ExprBuildError` | Parse or profile failure from a profile's `FromStr` implementation. |
| `EvalError` | Context, output representability, or output limits. |
| `IndexError` | Invalid extent, emitted index bounds, or numeric evaluation. |

Errors expose `source_text()`, `span()`, `kind()`, and `render()`. Profile and evaluation
errors also identify the originating `term_index()`. Error-kind types provide machine-readable
categories. `Span` is a half-open UTF-8 byte range in the expression source. `Display` on
an error renders a bounded source excerpt with a caret and explanation.

Scry decoding adds the configuration field's location and retains the concrete `ParseError`,
`ExprBuildError`, or `EvalError` as the `NodeError` cause. Primitive array conversion failures
keep their element location and original numeric cause.

Successful expressions retain their complete source. Errors normally retain it too. When input
exceeds its byte limit, `ParseError::source_text()` retains a UTF-8-safe prefix of at most
160 bytes. Its span still refers to the original input and may lie outside that prefix. This
also applies to a wrapping `ExprBuildError`. Use `render()` for bounded diagnostic display.
Do not assume the returned source text can be sliced using that span.

`IndexError` retains the underlying numeric error category as `IndexErrorKind::Evaluation`,
alongside the source text, span, and term association. It does not retain the original `EvalError`
as an `std::error::Error::source()` cause.

`evaluate()` either returns a complete vector or an error. `IntEvaluator::evaluate_each()`
instead invokes a callback with each retained `i64` value, its zero-based term index, and its
source span. It can invoke callbacks before a later term or callback fails.
The callback API is not transactional:

```rust
use scry::kit::seq_expr::{EvalError, IntEvalOptions, IntEvaluator, IntSeqExpr};

let expression: IntSeqExpr = "1,2,N".parse()?;
let mut received = Vec::new();
let result = IntEvaluator::new(IntEvalOptions::default()).evaluate_each(
    &expression,
    |value, _, _| {
        received.push(value);
        Ok::<_, EvalError>(())
    },
);
assert!(result.is_err());
assert_eq!(received, vec![1, 2]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Callers requiring atomic publication should use `evaluate()` or collect into a temporary value
and publish only after success. Preflighting one term's cardinality does not validate all later
terms. The callback's error type must implement `From<EvalError>`, allowing application errors
to retain numeric failures alongside their own validation failures.
