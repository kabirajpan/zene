use std::path::Path;
use std::process::Command;

/// Git context helper for querying repo status and uncommitted changes.
pub struct GitContext;

impl GitContext {
    /// Returns the current git branch name, if inside a git repository.
    pub fn current_branch(root: &Path) -> Option<String> {
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(root)
            .output()
            .ok()?;

        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !branch.is_empty() {
                return Some(branch);
            }
        }
        None
    }

    /// Returns a short porcelain status summary (modified, untracked files).
    pub fn status_summary(root: &Path) -> Option<String> {
        let output = Command::new("git")
            .args(["status", "--short"])
            .current_dir(root)
            .output()
            .ok()?;

        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !text.is_empty() {
                return Some(text);
            }
        }
        None
    }

    /// Returns a diff stat summary (`git diff --stat`) of uncommitted changes.
    pub fn diff_summary(root: &Path) -> Option<String> {
        let output = Command::new("git")
            .args(["diff", "--stat"])
            .current_dir(root)
            .output()
            .ok()?;

        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !text.is_empty() {
                return Some(text);
            }
        }
        None
    }

    /// Formats a concise git status block for prompt injection.
    pub fn format_for_prompt(root: &Path) -> Option<String> {
        let branch = Self::current_branch(root)?;
        let mut out = format!("### Git Context\n- Branch: `{}`\n", branch);

        if let Some(status) = Self::status_summary(root) {
            let lines: Vec<&str> = status.lines().collect();
            let count = lines.len();
            let sample = lines.iter().take(5).cloned().collect::<Vec<_>>().join("\n  ");
            out.push_str(&format!(
                "- Uncommitted Changes ({} files):\n  {}\n",
                count, sample
            ));
            if count > 5 {
                out.push_str(&format!("  ... ({} more files)\n", count - 5));
            }
        } else {
            out.push_str("- Working Tree: Clean\n");
        }

        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_context_in_current_repo() {
        let root = Path::new(".");
        let branch = GitContext::current_branch(root);
        assert!(branch.is_some(), "Should detect branch in current git repo");
        let formatted = GitContext::format_for_prompt(root);
        assert!(formatted.is_some());
        assert!(formatted.unwrap().contains("Branch: `"));
    }
}
