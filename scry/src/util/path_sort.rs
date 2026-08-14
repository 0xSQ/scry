//! Reusable path ordering.
//!
//! [`PathSort::Natural`] compares paired runs of ASCII digits by numeric magnitude, while
//! [`PathSort::Lexicographic`] treats digits as ordinary text. The difference is easiest to see
//! in paths containing numbers:
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
//! PathSort::Natural.sort(&mut natural);
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
//! Only ASCII digit runs receive numeric treatment. Both modes remain case-sensitive and
//! locale-independent.

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
/// conversion; natural numeric treatment applies only when both complete paths are Unicode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathSort {
    /// Compares paired ASCII digit runs by numeric magnitude.
    #[default]
    Natural,
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
                PathSort::Natural => compare_natural_text(left, right),
                PathSort::Lexicographic => left.cmp(right),
            }
        }
        (left, right) => left.cmp(&right),
    }
}

/// Compares component text with numeric treatment for paired ASCII digit runs.
fn compare_natural_text(left: &str, right: &str) -> Ordering {
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
    fn default_is_natural() {
        assert_eq!(PathSort::default(), PathSort::Natural);
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
        assert_path_order(PathSort::Natural, &["11", "2", "10", "1"], &["1", "2", "10", "11"]);
        assert_path_order(
            PathSort::Natural,
            &["v2.0", "v1.10", "v1.9"],
            &["v1.9", "v1.10", "v2.0"],
        );
        assert_path_order(PathSort::Natural, &["a2", "A10", "A2"], &["A2", "A10", "a2"]);
    }

    #[test]
    fn natural_path_order_pins_boundaries_and_delayed_ties() {
        assert_path_order(
            PathSort::Natural,
            &["a_1", "a01x", "a1", "a-1"],
            &["a-1", "a1", "a01x", "a_1"],
        );
        assert_eq!(compare_natural_text("a1w", "a01x"), Ordering::Less);
    }

    #[test]
    fn natural_path_order_uses_exact_component_ties_for_leading_zeroes() {
        assert_path_order(
            PathSort::Natural,
            &["image-1", "image-01", "image-001"],
            &["image-001", "image-01", "image-1"],
        );
        assert_path_order(PathSort::Natural, &["000", "0", "00"], &["0", "00", "000"]);
        assert_eq!(
            PathSort::Natural.compare(Path::new("d01/z"), Path::new("d1/a")),
            Ordering::Less
        );
    }

    #[test]
    fn path_order_is_component_aware() {
        for sort in [PathSort::Natural, PathSort::Lexicographic] {
            assert_path_order(sort, &["a0", "a/b", "a"], &["a", "a/b", "a0"]);
        }
    }

    #[test]
    fn natural_path_order_handles_unbounded_digit_runs() {
        let shorter = format!("file-{}", "9".repeat(80));
        let longer = format!("file-{}", "1".repeat(81));
        let mut paths = vec![PathBuf::from(&longer), PathBuf::from(&shorter)];

        PathSort::Natural.sort(&mut paths);

        assert_eq!(paths, vec![PathBuf::from(shorter), PathBuf::from(longer)]);
    }

    #[test]
    fn path_order_uses_unicode_scalars_without_normalization() {
        for sort in [PathSort::Natural, PathSort::Lexicographic] {
            assert_eq!(sort.compare(Path::new("\u{e000}"), Path::new("\u{10000}")), Ordering::Less);
            assert_eq!(sort.compare(Path::new("e\u{301}"), Path::new("\u{e9}")), Ordering::Less);
        }
        assert_eq!(compare_natural_text("a10", "a\u{0662}"), Ordering::Less);
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

        for sort in [PathSort::Natural, PathSort::Lexicographic] {
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

            for sort in [PathSort::Natural, PathSort::Lexicographic] {
                assert_eq!(compare_unicode_components(left, right, sort), left.cmp(&right));
            }
        }
    }

    #[test]
    fn sort_accepts_owned_and_borrowed_path_like_values() {
        let mut owned = vec![PathBuf::from("file-10"), PathBuf::from("file-2")];
        PathSort::Natural.sort(&mut owned);
        assert_eq!(owned, [PathBuf::from("file-2"), PathBuf::from("file-10")]);

        let mut borrowed = vec![Path::new("file-10"), Path::new("file-2")];
        PathSort::Natural.sort(&mut borrowed);
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
        for sort in [PathSort::Natural, PathSort::Lexicographic] {
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

        for sort in [PathSort::Natural, PathSort::Lexicographic] {
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

        for sort in [PathSort::Natural, PathSort::Lexicographic] {
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
        ];

        for sort in [PathSort::Natural, PathSort::Lexicographic] {
            assert_comparator_laws(&paths, sort);
        }
    }
}
