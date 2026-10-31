//! Reusable path ordering.
//!
//! [`PathSort::Natural`] compares paired runs of ASCII digits by numeric magnitude. Its
//! `dot_priority` option puts dots before other scalars within a normal path component. The
//! default enables this option, so `image37.png < image37-variant.png`. Disabling it retains
//! ordinary scalar ordering outside digit runs. [`PathSort::Lexicographic`] treats digits and
//! dots as ordinary text. The numeric difference is easiest to see in paths containing numbers:
//!
//! | Input paths               | Natural order             | Lexicographic order       |
//! | ------------------------- | ------------------------- | ------------------------- |
//! | `file-2`, `file-10`       | `file-2 < file-10`        | `file-10 < file-2`        |
//! | `v1.9`, `v1.10`           | `v1.9 < v1.10`            | `v1.10 < v1.9`            |
//! | `dir-9/x`, `dir-10/x`     | `dir-9/x < dir-10/x`      | `dir-10/x < dir-9/x`      |
//!
//! For example:
//!
//! ```
//! use scry::util::PathSort;
//! use std::path::PathBuf;
//!
//! let paths = vec![
//!     PathBuf::from("image-10.png"),
//!     PathBuf::from("image-2.png"),
//!     PathBuf::from("image-1.png"),
//! ];
//!
//! let mut natural = paths.clone();
//! PathSort::default().sort(&mut natural);
//! assert_eq!(
//!     natural,
//!     vec![
//!         PathBuf::from("image-1.png"),
//!         PathBuf::from("image-2.png"),
//!         PathBuf::from("image-10.png"),
//!     ]
//! );
//!
//! let mut lexicographic = paths;
//! PathSort::Lexicographic.sort(&mut lexicographic);
//! assert_eq!(
//!     lexicographic,
//!     vec![
//!         PathBuf::from("image-1.png"),
//!         PathBuf::from("image-10.png"),
//!         PathBuf::from("image-2.png"),
//!     ]
//! );
//! ```
//!
//! Dot priority applies to leading, intermediate, repeated, and trailing dots. Exhaustion still
//! sorts first, so `a < a.png`. Dots do not identify extensions or filename stems, and
//! `a.b.png < a.png` remains intentional. The same policy applies to file and directory names.
//!
//! Only ASCII digit runs receive numeric treatment. Equal numeric runs defer to the rest of the
//! component, so `a1w < a01x`. If complete scans tie, exact component spelling breaks the tie
//! before child components are compared, so `a001 < a01 < a1` and `d01/z < d1/a`.
//!
//! Both modes remain case-sensitive and locale-independent. Other Unicode scalars compare
//! exactly, without normalization. Structural path components retain Rust's standard ordering.

use std::cmp::Ordering;
use std::path::{Component, Path};

// ---------------------------------------------------------------------------------------------- //

/// Controls how paths are ordered.
///
/// Both modes compare [`Path::components`] lexicographically. Ordinary file and directory name
/// components use the selected text ordering. Structural components, such as roots, parent
/// directories, and Windows prefixes, use [`Component`]'s standard ordering.
///
/// Non-Unicode paths form a separate partition after all Unicode paths. Two non-Unicode paths use
/// Rust's native [`Path`] ordering. This gives arbitrary paths a total order without lossy text
/// conversion. Natural text treatment applies only when both complete paths are Unicode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathSort {
    /// Compares paired ASCII digit runs by numeric magnitude, with optional dot priority.
    ///
    /// Equal numeric runs defer to the remaining component text. Exact spelling resolves a
    /// complete component tie before child components are compared. Comparison remains
    /// case-sensitive and locale-independent, without interpreting filename extensions.
    Natural {
        /// Puts dots before other scalars within normal components.
        ///
        /// Defaults to `true`. Exhaustion still sorts first. Every dot receives priority, so
        /// `a.b.png < a.png`.
        /// Disabling this option preserves literal scalar ordering outside ASCII digit runs.
        dot_priority: bool,
    },
    /// Compares every scalar in normal path components exactly, including digits.
    Lexicographic,
}

impl PathSort {
    /// Compares two paths using this ordering mode.
    pub fn compare(self, left: &Path, right: &Path) -> Ordering {
        match (left.to_str(), right.to_str()) {
            (Some(_), Some(_)) => compare_unicode_paths(left, right, self),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => left.cmp(right),
        }
    }

    /// Sorts path-like values using this ordering mode.
    pub fn sort<T: AsRef<Path>>(self, paths: &mut [T]) {
        paths.sort_by(|left, right| self.compare(left.as_ref(), right.as_ref()));
    }
}

impl Default for PathSort {
    fn default() -> Self {
        Self::Natural { dot_priority: true }
    }
}

/// Compares Unicode paths component by component.
fn compare_unicode_paths(left: &Path, right: &Path, sort: PathSort) -> Ordering {
    let mut left_components = left.components();
    let mut right_components = right.components();

    loop {
        match (left_components.next(), right_components.next()) {
            (Some(left), Some(right)) => {
                let order = compare_unicode_components(left, right, sort);
                if order != Ordering::Equal {
                    return order;
                }
            }
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (None, None) => return Ordering::Equal,
        }
    }
}

/// Compares one pair of components under the selected text mode.
fn compare_unicode_components(
    left: Component<'_>,
    right: Component<'_>,
    sort: PathSort,
) -> Ordering {
    match (left, right) {
        (Component::Normal(left), Component::Normal(right)) => {
            let left = left.to_str().expect("Unicode path has a non-Unicode component");
            let right = right.to_str().expect("Unicode path has a non-Unicode component");
            match sort {
                PathSort::Natural { dot_priority } => {
                    compare_natural_text(left, right, dot_priority)
                }
                PathSort::Lexicographic => left.cmp(right),
            }
        }
        (left, right) => left.cmp(&right),
    }
}

/// Compares component text with numeric treatment and optional dot priority.
fn compare_natural_text(left: &str, right: &str, dot_priority: bool) -> Ordering {
    let original_left = left;
    let original_right = right;
    let mut left = left;
    let mut right = right;

    loop {
        match (left.is_empty(), right.is_empty()) {
            (true, true) => return original_left.cmp(original_right),
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            (false, false) => {}
        }

        let left_byte = left.as_bytes()[0];
        let right_byte = right.as_bytes()[0];
        if left_byte.is_ascii_digit() && right_byte.is_ascii_digit() {
            let left_run_len = left.bytes().take_while(u8::is_ascii_digit).count();
            let right_run_len = right.bytes().take_while(u8::is_ascii_digit).count();
            let order = compare_digit_runs(&left[..left_run_len], &right[..right_run_len]);
            if order != Ordering::Equal {
                return order;
            }
            left = &left[left_run_len..];
            right = &right[right_run_len..];
            continue;
        }

        if dot_priority && (left_byte == b'.') != (right_byte == b'.') {
            return if left_byte == b'.' {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }

        let left_char = left.chars().next().expect("nonempty component has a scalar");
        let right_char = right.chars().next().expect("nonempty component has a scalar");
        let order = left_char.cmp(&right_char);
        if order != Ordering::Equal {
            return order;
        }
        left = &left[left_char.len_utf8()..];
        right = &right[right_char.len_utf8()..];
    }
}

/// Compares two ASCII digit runs by unbounded numeric magnitude.
fn compare_digit_runs(left: &str, right: &str) -> Ordering {
    let left = left.trim_start_matches('0');
    let right = right.trim_start_matches('0');
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[cfg(any(unix, windows))]
    use std::ffi::OsString;
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;
    #[cfg(windows)]
    use std::os::windows::ffi::OsStringExt;

    // ------------------------------------------------------------------------------------------ //

    fn ordering_modes() -> [PathSort; 3] {
        [
            PathSort::Natural { dot_priority: true },
            PathSort::Natural {
                dot_priority: false,
            },
            PathSort::Lexicographic,
        ]
    }

    fn assert_path_order(sort: PathSort, input: &[&str], expected: &[&str]) {
        let mut paths: Vec<PathBuf> = input.iter().map(PathBuf::from).collect();
        sort.sort(&mut paths);
        let expected: Vec<PathBuf> = expected.iter().map(PathBuf::from).collect();
        assert_eq!(paths, expected);
    }

    fn assert_comparator_laws(paths: &[PathBuf], sort: PathSort) {
        for left in paths {
            for right in paths {
                let order = sort.compare(left, right);
                assert_eq!(order, sort.compare(right, left).reverse());
                assert_eq!(order == Ordering::Equal, left == right);

                for third in paths {
                    if order != Ordering::Greater && sort.compare(right, third) != Ordering::Greater
                    {
                        assert_ne!(sort.compare(left, third), Ordering::Greater);
                    }
                }
            }
        }
    }

    #[cfg(unix)]
    fn opaque_component(tag: u8) -> OsString {
        OsString::from_vec(vec![b'o', 0xff, tag])
    }

    #[cfg(windows)]
    fn opaque_component(tag: u8) -> OsString {
        OsString::from_wide(&[b'o' as u16, 0xd800, tag as u16])
    }

    #[test]
    fn default_is_natural_with_dot_priority() {
        assert_eq!(PathSort::default(), PathSort::Natural { dot_priority: true });
        assert_path_order(
            PathSort::default(),
            &[
                "image100.png",
                "image37-variant.png",
                "image10.png",
                "image37.png",
                "image2.png",
            ],
            &[
                "image2.png",
                "image10.png",
                "image37.png",
                "image37-variant.png",
                "image100.png",
            ],
        );
    }

    #[test]
    fn natural_dot_priority_is_optional_and_lexicographic_order_stays_literal() {
        let prioritized = PathSort::Natural { dot_priority: true };
        let literal = PathSort::Natural {
            dot_priority: false,
        };
        for (base, variant) in [
            ("image37.png", "image37-variant.png"),
            ("a.png", "a-foo.png"),
            ("a.png", "a!.png"),
            ("a.png", "a .png"),
            (".image2", "!image2"),
        ] {
            assert_eq!(prioritized.compare(Path::new(base), Path::new(variant)), Ordering::Less);
            for sort in [literal, PathSort::Lexicographic] {
                assert_eq!(sort.compare(Path::new(base), Path::new(variant)), Ordering::Greater);
            }
        }
        assert_path_order(literal, &["a-10.png", "a-2.png"], &["a-2.png", "a-10.png"]);
        assert_path_order(
            PathSort::Lexicographic,
            &["a-2.png", "a-10.png"],
            &["a-10.png", "a-2.png"],
        );
    }

    #[test]
    fn dot_priority_does_not_interpret_extensions_or_ignore_dots() {
        let sort = PathSort::Natural { dot_priority: true };
        assert_path_order(
            sort,
            &["a.png", "a.b.png", "a..png", "a.", "a"],
            &["a", "a.", "a..png", "a.b.png", "a.png"],
        );
        assert_path_order(sort, &[".image10", ".image2"], &[".image2", ".image10"]);
        assert_path_order(sort, &["a10.", "a2..", "a2."], &["a2.", "a2..", "a10."]);
        assert_path_order(sort, &["v1.10", "v1.9.0", "v1.9"], &["v1.9", "v1.9.0", "v1.10"]);
        assert_path_order(sort, &["dir-variant/a", "dir.png/z"], &["dir.png/z", "dir-variant/a"]);
    }

    #[test]
    fn dot_priority_preserves_case_and_numeric_boundaries() {
        for dot_priority in [false, true] {
            let sort = PathSort::Natural { dot_priority };
            assert_path_order(sort, &["a8", "A5", "a2"], &["A5", "a2", "a8"]);
            assert_path_order(sort, &["ä2.png", "Ä2.png"], &["Ä2.png", "ä2.png"]);
            assert_path_order(sort, &["a_1", "a1", "a-1", "a!1"], &["a!1", "a-1", "a1", "a_1"]);
            assert_path_order(sort, &["aa.png", "a0.png", "a.png"], &["a.png", "a0.png", "aa.png"]);
            let expected = if dot_priority {
                ["a1.x", "a01-x"]
            } else {
                ["a01-x", "a1.x"]
            };
            assert_path_order(sort, &["a01-x", "a1.x"], &expected);
        }
    }

    #[test]
    fn lexicographic_path_order_treats_digits_as_text() {
        assert_path_order(
            PathSort::Lexicographic,
            &["11", "2", "10", "1"],
            &["1", "10", "11", "2"],
        );
    }

    #[test]
    fn natural_path_order_compares_ascii_digit_runs_by_magnitude() {
        for dot_priority in [false, true] {
            let sort = PathSort::Natural { dot_priority };
            assert_path_order(sort, &["11", "2", "10", "1"], &["1", "2", "10", "11"]);
            assert_path_order(sort, &["v2.0", "v1.10", "v1.9"], &["v1.9", "v1.10", "v2.0"]);
            assert_path_order(sort, &["a2", "A10", "A2"], &["A2", "A10", "a2"]);
        }
    }

    #[test]
    fn natural_path_order_pins_boundaries_and_delayed_ties() {
        for dot_priority in [false, true] {
            let sort = PathSort::Natural { dot_priority };
            assert_path_order(sort, &["a_1", "a01x", "a1", "a-1"], &["a-1", "a1", "a01x", "a_1"]);
            assert_eq!(sort.compare(Path::new("a1w"), Path::new("a01x")), Ordering::Less);
        }
    }

    #[test]
    fn natural_path_order_uses_exact_component_ties_for_leading_zeroes() {
        for dot_priority in [false, true] {
            let sort = PathSort::Natural { dot_priority };
            assert_path_order(
                sort,
                &["image-1", "image-01", "image-001"],
                &["image-001", "image-01", "image-1"],
            );
            assert_path_order(sort, &["000", "0", "00"], &["0", "00", "000"]);
            assert_path_order(
                sort,
                &["a1.png", "a01.png", "a001.png"],
                &["a001.png", "a01.png", "a1.png"],
            );
            assert_eq!(sort.compare(Path::new("d01/z"), Path::new("d1/a")), Ordering::Less);
        }
    }

    #[test]
    fn path_order_is_component_aware() {
        for sort in ordering_modes() {
            assert_path_order(sort, &["a0", "a/b", "a"], &["a", "a/b", "a0"]);
        }
    }

    #[test]
    fn natural_path_order_handles_unbounded_digit_runs() {
        let shorter = format!("file-{}", "9".repeat(80));
        let longer = format!("file-{}", "1".repeat(81));
        for dot_priority in [false, true] {
            let mut paths = vec![PathBuf::from(&longer), PathBuf::from(&shorter)];
            PathSort::Natural { dot_priority }.sort(&mut paths);
            assert_eq!(paths, vec![PathBuf::from(&shorter), PathBuf::from(&longer)]);
        }
    }

    #[test]
    fn path_order_uses_unicode_scalars_without_normalization() {
        for sort in ordering_modes() {
            assert_eq!(sort.compare(Path::new("\u{e000}"), Path::new("\u{10000}")), Ordering::Less);
            assert_eq!(sort.compare(Path::new("e\u{301}"), Path::new("\u{e9}")), Ordering::Less);
        }
        for dot_priority in [false, true] {
            assert_eq!(
                PathSort::Natural { dot_priority }
                    .compare(Path::new("a10"), Path::new("a\u{0662}")),
                Ordering::Less
            );
        }
    }

    #[test]
    fn path_order_delegates_special_components_to_rust() {
        let paths = [
            Path::new(std::path::MAIN_SEPARATOR_STR),
            Path::new("."),
            Path::new(".."),
            Path::new("a"),
        ];
        let components: Vec<Component<'_>> =
            paths.iter().map(|path| path.components().next().unwrap()).collect();

        for sort in ordering_modes() {
            for left in &components {
                for right in &components {
                    assert_eq!(compare_unicode_components(*left, *right, sort), left.cmp(right));
                }
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn path_order_delegates_windows_prefixes_to_rust() {
        for (left_path, right_path) in [(r"C:\a", r"D:\a"), (r"\\.\COM2\a", r"\\.\COM10\a")] {
            let left = Path::new(left_path).components().next().unwrap();
            let right = Path::new(right_path).components().next().unwrap();

            for sort in ordering_modes() {
                assert_eq!(compare_unicode_components(left, right, sort), left.cmp(&right));
            }
        }
    }

    #[test]
    fn sort_accepts_owned_and_borrowed_path_like_values() {
        let mut owned = vec![PathBuf::from("file-10"), PathBuf::from("file-2")];
        PathSort::default().sort(&mut owned);
        assert_eq!(owned, [PathBuf::from("file-2"), PathBuf::from("file-10")]);

        let mut borrowed = vec![Path::new("file-10"), Path::new("file-2")];
        PathSort::default().sort(&mut borrowed);
        assert_eq!(borrowed, [Path::new("file-2"), Path::new("file-10")]);
    }

    #[test]
    fn path_comparator_obeys_total_order_laws_and_ignores_input_permutation() {
        let paths: Vec<PathBuf> = [
            "",
            std::path::MAIN_SEPARATOR_STR,
            ".",
            "..",
            "a",
            "a/b",
            "a0",
            "0",
            "00",
            "2",
            "10",
            "a-1",
            "a1",
            "a01x",
            "a1w",
            "a_1",
            "a.",
            "a..",
            "a.png",
            "a.b.png",
            "a..png",
            "a!.png",
            "a .png",
            "a-foo.png",
            "a0.png",
            "a00.png",
            "a01.x",
            "a1!x",
            "a01-x",
            "a1.x",
            ".image2",
            ".image10",
            "!image2",
            "d.1/z",
            "d-1/a",
            "d01/z",
            "d1/a",
            "a10",
            "a\u{0662}",
            "\u{e000}",
            "\u{10000}",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect();
        for sort in ordering_modes() {
            assert_comparator_laws(&paths, sort);

            let mut expected = paths.clone();
            sort.sort(&mut expected);

            let mut reversed = paths.clone();
            reversed.reverse();
            sort.sort(&mut reversed);
            assert_eq!(reversed, expected);

            let mut rotated = paths.clone();
            rotated.rotate_left(7);
            sort.sort(&mut rotated);
            assert_eq!(rotated, expected);
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn unicode_paths_sort_before_opaque_paths() {
        let unicode = PathBuf::from("zzzz");
        let opaque = PathBuf::from(opaque_component(b'a'));

        for sort in ordering_modes() {
            assert_eq!(sort.compare(&unicode, &opaque), Ordering::Less);
            assert_eq!(sort.compare(&opaque, &unicode), Ordering::Greater);
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn opaque_path_order_matches_native_order_and_preserves_distinctions() {
        let left = PathBuf::from(opaque_component(b'a'));
        let right = PathBuf::from(opaque_component(b'b'));
        assert_ne!(left, right);

        for sort in ordering_modes() {
            assert_eq!(sort.compare(&left, &right), left.cmp(&right));
            assert_ne!(sort.compare(&left, &right), Ordering::Equal);
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn mixed_unicode_and_opaque_paths_obey_total_order_laws() {
        let paths = vec![
            PathBuf::from("file-10"),
            PathBuf::from(opaque_component(b'b')),
            PathBuf::from("file-2"),
            PathBuf::from(opaque_component(b'a')),
            PathBuf::from("file.png"),
            PathBuf::from("file-variant.png"),
        ];

        for sort in ordering_modes() {
            assert_comparator_laws(&paths, sort);
        }
    }
}
