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
ranges. Concatenation preserves order and repeated occurrences:

```text
3..10:2   -> 3, 5, 7, 9
10..3:2   -> 10, 8, 6, 4
2,1,2     -> 2, 1, 2
```

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
[1..0]/4       -> 1.0, 0.75, 0.5, 0.25, 0.0  (real profile)
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

Integer endpoints, steps, and sampling calculations use exact arithmetic. Only retained values
must fit the output type. A range can therefore use an excluded stop beyond that type's maximum,
or subdivide very large coordinates to produce a small interior value. For example,
`(-9223372036854775809..9223372036854775809)/2` emits just `0`, which fits `i64`.
An empty range has no values to convert. Required context and output count limits still apply.

### Exact real anchors

Decimal literals, endpoint comparisons, cardinalities, and real anchors use exact arithmetic.
`[0..0.3]:0.1` produces four anchors, including the stop. Repeated floating addition does not
determine whether the last anchor is retained.

Each retained real anchor is rounded once to the requested `f32` or `f64` using nearest rounding
with ties to even. `f32` evaluation converts the exact anchor directly to `f32`.
Results are finite and monotonic in the range's direction. Floating zeros are normalized to
positive zero. Underflow, duplicate floats, and aliases of excluded endpoints are ordinary
rounding outcomes. For example, `[0..1e-1000]/2` produces three zeros.

An emitted value that cannot become finite in the requested type is an evaluation error.
Endpoints and step magnitudes need not themselves fit that type when every retained result does.

## Rust API and numeric profiles

The Rust API separates parsing from evaluation. An expression retains its authored text, and
evaluation returns an ordinary vector of numbers or indices. Choose the numeric profile for
the values your application needs:

| Type | Validation while parsing | Evaluation |
| --- | --- | --- |
| `SeqExpr` | Grammar and parser limits. | Convert to an integer or real profile first. |
| `IntSeqExpr` | Grammar, limits, and integer-spelled literals and steps. | `IntEvaluator` or `IndexEvaluator`. |
| `RealSeqExpr` | Grammar and limits. Ranges require both endpoints and explicit sampling. | `RealEvaluator`. |

All three types implement `FromStr`. `SeqExpr::parse_with_limits` accepts explicit parser limits.
`IntSeqExpr::try_from` and `RealSeqExpr::try_from` validate an already parsed `SeqExpr`.
The profile types expose the underlying expression through `as_expr()`.

```rust
use scry::kit::seq_expr::{
    IndexEvaluator, IntEvalOptions, IntEvaluator, IntSeqExpr, RealEvalOptions, RealEvaluator,
    RealSeqExpr,
};

let integers: IntSeqExpr = "[0..10]/4".parse()?;
let values = IntEvaluator::new(IntEvalOptions::default()).evaluate(&integers)?;
assert_eq!(values, vec![0, 3, 5, 8, 10]);

let reals: RealSeqExpr = "[0..0.3]:0.1".parse()?;
let real_evaluator = RealEvaluator::new(RealEvalOptions::default());
let values = real_evaluator.evaluate(&reals)?;
assert_eq!(values, vec![0.0, 0.1, 0.2, 0.3]);
let values: Vec<f32> = real_evaluator.evaluate_f32(&reals)?;
assert_eq!(values, vec![0.0, 0.1, 0.2, 0.3]);

let selection: IntSeqExpr = "N-3..".parse()?;
let indices = IndexEvaluator::default().evaluate(&selection, 12)?;
assert_eq!(indices, vec![9, 10, 11]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`source()` and `Display` preserve the source exactly, including whitespace and numeric spelling.
They do not format a canonical expression or display evaluated values:

```rust
use scry::kit::seq_expr::{IntSeqExpr, SeqExpr};

let authored = "  +1, [2..6]:2  ";
let parsed: SeqExpr = authored.parse()?;
let integers = IntSeqExpr::try_from(parsed)?;
assert_eq!(integers.source(), authored);
assert_eq!(integers.to_string(), authored);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Profile validation does not evaluate coordinates or convert real anchors to floats. An integer
literal outside the target integer range, or a real literal outside the target's finite range,
can therefore parse successfully and fail when evaluation needs that value. `IntEvaluator`
returns `Vec<i64>`. `RealEvaluator::evaluate` returns `Vec<f64>`, and `evaluate_f32` returns
`Vec<f32>`. Via policies select the numeric type from the field declaration. An integer
expression containing `N` or an omitted endpoint can be retained before its context exists.

## Scry configuration

Expression types can be fields in an ordinary Scry configuration. They accept a string and
retain its source for later evaluation. A plain numeric `Vec` field accepts an array of values.
For example, an experiment can store concrete iteration values alongside an expression that
selects their positions and a real-valued sweep:

```rust
use scry::kit::seq_expr::{IndexEvaluator, IntSeqExpr, RealSeqExpr};
use scry::node::Format;
use scry::{Config, Node, ToNode};

#[derive(Debug, Config, ToNode)]
struct Experiment {
    /// Concrete iteration values.
    iterations: Vec<i64>,
    /// Positions to select when the input list is available.
    #[scry(default = "..".parse::<IntSeqExpr>().expect("valid default selector"))]
    select: IntSeqExpr,
    /// An optional second selection for comparison.
    comparison: Option<IntSeqExpr>,
    /// A sweep to evaluate when the experiment runs.
    strengths: RealSeqExpr,
}

let input =
    r#"#{ iterations: [10, "20", 30], select: " N-2.. ", strengths: "[0..0.3]:0.1" }"#;
let config: Experiment = Node::parse_str(input, Format::Rhai)?.as_type()?;

let indices = IndexEvaluator::default().evaluate(&config.select, config.iterations.len())?;
assert_eq!(indices, vec![1, 2]);

let output = config.to_node()?;
assert_eq!(output.req::<Vec<i64>>("iterations")?, vec![10, 20, 30]);
assert_eq!(output.req::<String>("select")?, " N-2.. ");
assert_eq!(output.req::<String>("strengths")?, "[0..0.3]:0.1");
assert_eq!(output.req::<Option<String>>("comparison")?, None);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Decoding parses the expression strings and checks their numeric profiles. It does not expand
them. The selector can therefore contain `N` before a source length exists. Apply configuration
overrides before decoding, then supply the final collection length when evaluating the selector.
The same retained selector can be reused against several collections.

Explicit `ToNode` output writes numeric arrays for the concrete values, preserves expression
strings exactly, and writes the absent comparison as null. The source spaces around `N-2..`
survive even though they make no difference to evaluation.

Native arrays follow Scry's ordinary primitive conversions. The iteration array above accepts
`"20"` as an integer. Array elements do not expand sequence terms, so `["3..7"]` fails to
decode as `Vec<i64>`. A whole expression string also cannot decode as a plain `Vec<i64>`.

Missing and null values follow the field's declared type and fallback:

- `iterations` and `strengths` are required. Missing either key is an error.
- Missing `select` uses the explicitly parsed `".."` default. A present null is still invalid.
- Missing or null `comparison` becomes `None`. A supplied expression string becomes `Some`.

Expression types accept only string values. They have no inherent Rust `Default` or Scry
`FromDefaults`, because there is no expression that serves as a default in every context.
An explicit field default chooses the target expression only when its key is missing.

Scry descriptions identify the accepted values as `sequence expression string`,
`integer sequence expression string`, or `real sequence expression string`. Invalid syntax or
profile errors carry the configuration field's location and retain the concrete `ParseError`
or `ExprBuildError` as their cause. The authored span and error kind remain available on that
cause.

These field shapes work across Scry-supported configuration formats. The executable example
uses Rhai, which is available without format feature flags. An equivalent JSON document uses
the same arrays and expression strings when `format-json` is enabled.

### Expanding expressions into vectors

When a field needs the numeric values immediately, use a Via sequence policy. `IntSequence`
adapts an entire integer vector field, and `RealSequence` adapts an entire floating-point vector.
Each accepts either an expression string or a numeric array. Decoding an expression evaluates
it into the field's declared element type:

```rust
use scry::kit::seq_expr::{IntSequence, RealSequence};
use scry::node::Format;
use scry::{Config, Node, ToNode};

#[derive(Debug, Config, ToNode)]
struct Sweep {
    #[scry(via(IntSequence))]
    iterations: Vec<u32>,
    #[scry(via(RealSequence))]
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

Both inputs produce the same vectors. Explicit `ToNode` output always writes numeric arrays.
The vectors retain the values, order, and duplicates. They do not retain the expression's
source or which input form was used.

| Policy | Supported vector element types |
| --- | --- |
| `IntSequence` | `i8`, `i16`, `i32`, `i64`, `isize`, `u8`, `u16`, `u32`, `u64`, `usize`. |
| `RealSequence` | `f32`, `f64`. |

Integer expressions use the full range of the declared type. For a `Vec<u64>`, the expression
`"18446744073709551615..18446744073709551616"` emits `[u64::MAX]`. The excluded upper boundary
does not need to fit `u64`. Negative retained values fail for unsigned fields, and every retained
value must fit the target. These conversions report the target type in their evaluation error.
`isize` and `usize` use the current platform's width.

Immediate expansion uses the default parser and evaluator limits and supplies no integer
context. Expressions such as `"N-1"` and `".."` fail during decoding. The policies do not infer
context from neighboring fields or arrays. Keep an `IntSeqExpr` when evaluation needs a source
length or explicit bounds supplied later.

Array entries use Scry's primitive conversions. Numeric strings such as `"3"` are accepted,
but `"3..7"` inside an array is not expanded. Integer arrays reject floating-point values.
Real arrays convert values as the format reader supplied them. Expression strings retain exact
decimal anchors until rounding to the field's type.
Empty arrays are valid empty vectors. Empty expression strings are invalid, while a valid
expression that emits no values produces an empty vector.

`RealSequence` requires every decoded or serialized value to be finite, including values
supplied as native arrays. Native array entries such as `"NaN"` or `"inf"` are rejected.
Signed zero is preserved in arrays. Expression-derived zero keeps the evaluator's positive-zero
normalization. In arrays, conversion and finiteness failures identify the offending element.

Each policy permits at most `MAX_VALUES` values on input and output. This also checks vectors
constructed directly in Rust before serializing them. Parser byte and literal limits apply to
expression strings. The array count check bounds construction of the policy result, after the
format reader has already built the input Node array.

These policies preserve ordinary field defaults. Missing required keys fail, explicit defaults
apply only to missing keys, and a direct sequence policy rejects null. A target vector supplied
by a field default is used directly rather than parsed as an expression.

### Optional and grouped sequences

The existing Via container policies compose with sequence policies. Use `Option<RealSequence>`
for an optional vector, or `Vec<IntSequence>` to adapt each entry in a list of integer vectors:

```rust
use scry::kit::seq_expr::{IntSequence, RealSequence};
use scry::node::Format;
use scry::{Config, Node, ToNode};

#[derive(Debug, Config, ToNode)]
struct Groups {
    #[scry(via(Option<RealSequence>))]
    strengths: Option<Vec<f64>>,
    #[scry(via(Vec<IntSequence>))]
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

An omitted or null `strengths` becomes `None`. A supplied array or expression produces a vector
stored in `Some`. Group entries can independently use either input form, and output contains
numeric arrays for every group. The sequence ceiling applies to each inner vector. The outer
list follows the ordinary Via vector rules.

Descriptions use the hints `integer sequence expression string or integer array` and
`real sequence expression string or real array`. They describe the forms without evaluating
an expression. Scry's CLI `--get` queries the original Node before typed conversion,
so it reports an authored expression string when that is the input. Expanded arrays appear
when the application explicitly serializes its decoded values with `ToNode`. Child-index
queries through `--get` are available when the original input is an array.

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

### End-relative values

Uppercase `N` is the source length and the exclusive upper index boundary. The final valid index
is `N-1`. Optional `+` or `-` offsets take an unsigned ASCII decimal magnitude. Whitespace,
zero offsets, and leading zeros are allowed, including `N - 6`, `N+0`, and `N-0006`.

Use symbolic values in integer singletons or endpoints. Steps and counts remain numeric.
`0..N:2` is valid. `0..N:N-1` and `0..N/N` are invalid.

Lowercase `n`, `+N`, `-N`, `N--1`, `N+-1`, missing magnitudes, decimal or exponent offsets,
digit separators, chained offsets, multiplication, and grouped arithmetic are invalid.

Offsets are resolved exactly. The written magnitude and range endpoints need not fit the output
type. Only retained values are narrowed. For `IntEvaluator`, evaluating `N+1` at length
`i64::MAX` is an `i64` range error, while the empty range `N+1..N+1` succeeds.

Direction follows the resolved endpoints. `N-6..0` descends at length 10 and ascends at length 4.
Resolve required context before deciding that a term is empty. Even `N..N` needs finite-source
context. Numeric `FiniteSource` lengths use the full `usize` range. `IndexEvaluator` separately
requires its length to fit `i64`.

### Strict indices

`IndexEvaluator` validates every emitted integer against `0 <= index < length`, including
rounded subdivision results. It preserves order and repeated occurrences. It does not clip
negative coordinates, unavailable suffixes, or oversized ranges.

| Expression at length 12 | Result |
| --- | --- |
| `..` | Indices 0 through 11. |
| `..:3` | `[0, 3, 6, 9]`. |
| `2,0,2` | `[2, 0, 2]`. |
| `N-2..` | `[10, 11]`. |
| `(N..0]` | Indices 11 through 0, descending. |
| `..100` | Error when an emitted index reaches 12. |
| `100..` | Error at the first emitted index, 100. |
| `[0..]` | Error because the included contextual stop is 12. |
| `-1` or `N` | Bounds error. |
| `N-13..` | Error at -1. |
| `0..100:100` | `[0]`. The unvisited boundary does not invalidate it. |
| `N-1..N+1:2` | `[11]`. |
| `[0..1]/4` | `[0, 0, 1, 1, 1]`. |

At length 10, `[N-1..N)/2` retains exact anchors 9 and 9.5, then fails because 9.5 rounds
to index 10. To sample a nonempty source including its final valid position, use `[0..N-1]/4`.

Lengths use `usize` and must also fit `i64`. Empty results are allowed. At length zero, `..`
and `N..N` produce no indices. `N..N/2` retains repeated anchors and fails at index zero.
The same parsed expression can be evaluated separately against different source lengths.

## Limits

| Resource | Hard maximum |
| --- | ---: |
| Expression UTF-8 bytes | 4 MiB, or 4,194,304 bytes. |
| Decimal digits in one numeric, count, or offset token | 1,024. |
| Absolute written exponent | 4,096. |
| Values emitted by one evaluation | 1,000,000, exposed as `MAX_VALUES`. |
| Numeric finite source length | `usize::MAX`. |
| Index selection length | `i64::MAX`, also constrained by `usize`. |

`ParseLimits`, `IntEvalOptions::max_values`, `RealEvalOptions::max_values`, and
`IndexEvaluator::new(max_values)` can tighten the relevant ceilings. Larger requested limits
are capped. The input byte limit bounds comma-separated terms without a separate term limit.
Literal digit limits count leading zeros, significand digits, and exponent digits. Offset
magnitudes have no exponent syntax.

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
categories. `Span` is a half-open UTF-8 byte range in the authored expression. `Display` on
an error renders a bounded source excerpt with a caret and explanation.

Successful expressions retain their complete source. Errors normally retain it too. When input
exceeds its byte limit, `ParseError::source_text()` retains a UTF-8-safe prefix of at most
160 bytes. Its span still refers to the original input and may lie outside that prefix. This
also applies to a wrapping `ExprBuildError`. Use `render()` for bounded diagnostic display.
Do not assume the returned source text can be sliced using that span.

`IndexError` retains the underlying numeric error category as `IndexErrorKind::Evaluation`,
alongside the source text, span, and term association. It does not retain the original `EvalError` as an
`std::error::Error::source()` cause.

`evaluate()` either returns a complete vector or an error. `IntEvaluator::evaluate_each()`
instead invokes a callback for each bounded candidate, with its integer value, zero-based term
index, and source span. It can invoke callbacks before a later term or callback fails.
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
