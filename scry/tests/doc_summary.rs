//! The derive keeps only a doc comment's first paragraph as the description.
//!
//! Rustdoc convention: the first paragraph is the summary, later paragraphs are elaboration for
//! source readers. Descriptions (`--desc`, error field listings) show only the summary.

use scry::Describe;

/// Test struct whose docs mix summaries with elaboration.
#[derive(Debug, scry::FromNode, scry::Describe)]
#[allow(dead_code)] // Only the description output is inspected; the fields are never read.
struct Documented {
    /// Short field summary.
    ///
    /// Long field elaboration meant for source readers only, with enough detail that it would
    /// visibly bloat a description line.
    field: String,
    /// A two-line summary that wraps
    /// across source lines without a break.
    wrapped: String,
}

#[test]
fn descriptions_keep_only_the_first_doc_paragraph() {
    let output = Documented::describe().display();

    assert!(output.contains("Short field summary."), "was: {output}");
    assert!(!output.contains("elaboration"), "was: {output}");
    assert!(
        output.contains("A two-line summary that wraps across source lines without a break."),
        "was: {output}"
    );
}
