//! Grouped block diff generator.
//! Formats diffs with all removed lines grouped first, followed by all added lines,
//! rather than alternating line-by-line interleaving.

/// Represents a grouped diff result with line metrics and formatted output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupedDiff {
    pub lines_added: usize,
    pub lines_removed: usize,
    pub formatted: String,
}

impl GroupedDiff {
    /// Formats a replacement diff grouping all `-` lines together first,
    /// followed by all `+` lines together.
    ///
    /// Example:
    /// ```text
    /// - line 1
    /// - line 2
    /// - line 3
    /// + line 1
    /// + line 2
    /// + line 3
    /// ```
    pub fn from_replacement(target: &str, replacement: &str) -> Self {
        let mut out = String::new();
        let mut removed = 0;
        let mut added = 0;

        for line in target.lines() {
            out.push_str(&format!("- {}\n", line));
            removed += 1;
        }

        for line in replacement.lines() {
            out.push_str(&format!("+ {}\n", line));
            added += 1;
        }

        Self {
            lines_added: added,
            lines_removed: removed,
            formatted: out,
        }
    }
}

/// Helper function to generate grouped diff string directly.
pub fn format_grouped_diff(target: &str, replacement: &str) -> String {
    GroupedDiff::from_replacement(target, replacement).formatted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grouped_diff_formatting() {
        let target = "line 1\nline 2\nline 3";
        let replacement = "line 1\nline 2\nline 3";

        let diff = GroupedDiff::from_replacement(target, replacement);

        let expected = "- line 1\n- line 2\n- line 3\n+ line 1\n+ line 2\n+ line 3\n";
        assert_eq!(diff.formatted, expected);
        assert_eq!(diff.lines_removed, 3);
        assert_eq!(diff.lines_added, 3);
    }

    #[test]
    fn test_grouped_diff_empty_target() {
        let diff = GroupedDiff::from_replacement("", "new line 1\nnew line 2");
        assert_eq!(diff.lines_removed, 0);
        assert_eq!(diff.lines_added, 2);
        assert_eq!(diff.formatted, "+ new line 1\n+ new line 2\n");
    }

    #[test]
    fn test_grouped_diff_empty_replacement() {
        let diff = GroupedDiff::from_replacement("deleted line", "");
        assert_eq!(diff.lines_removed, 1);
        assert_eq!(diff.lines_added, 0);
        assert_eq!(diff.formatted, "- deleted line\n");
    }
}
