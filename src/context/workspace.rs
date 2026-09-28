use std::path::{Path, PathBuf};

/// Information about an open editor file in the IDE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFileInfo {
    pub path: PathBuf,
    pub is_active: bool,
    pub cursor_line: Option<usize>,
    pub cursor_col: Option<usize>,
    pub cursor_slice: Option<String>,
}

impl OpenFileInfo {
    pub fn new(path: PathBuf, is_active: bool) -> Self {
        Self {
            path,
            is_active,
            cursor_line: None,
            cursor_col: None,
            cursor_slice: None,
        }
    }

    /// Extracts a slice of code centered around the given cursor line (1-indexed).
    pub fn with_cursor(
        mut self,
        line: usize,
        col: usize,
        file_content: &str,
        window_radius: usize,
    ) -> Self {
        self.cursor_line = Some(line);
        self.cursor_col = Some(col);

        let lines: Vec<&str> = file_content.lines().collect();
        if lines.is_empty() {
            return self;
        }

        // Convert 1-indexed line to 0-indexed
        let target_idx = line.saturating_sub(1).min(lines.len() - 1);
        let start_idx = target_idx.saturating_sub(window_radius);
        let end_idx = (target_idx + window_radius + 1).min(lines.len());

        let mut slice = String::new();
        for i in start_idx..end_idx {
            let line_num = i + 1;
            let marker = if line_num == line { ">" } else { " " };
            slice.push_str(&format!("{:4} {} {}\n", line_num, marker, lines[i]));
        }

        self.cursor_slice = Some(slice);
        self
    }
}

/// Context engine capturing the state of the workspace and editor tabs.
#[derive(Debug, Clone)]
pub struct WorkspaceContext {
    pub root: PathBuf,
    pub open_files: Vec<OpenFileInfo>,
}

impl WorkspaceContext {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            open_files: Vec::new(),
        }
    }

    pub fn with_open_files(mut self, open_files: Vec<OpenFileInfo>) -> Self {
        self.open_files = open_files;
        self
    }

    pub fn active_file(&self) -> Option<&OpenFileInfo> {
        self.open_files.iter().find(|f| f.is_active)
    }

    /// Formats the workspace and active editor context into a compact markdown block
    /// suitable for injecting into the agent's prompt.
    pub fn format_for_prompt(&self) -> String {
        let mut out = String::new();
        out.push_str("### Workspace & Editor Context\n");
        out.push_str(&format!("- Workspace Root: `{}`\n", self.root.display()));

        if let Some(active) = self.active_file() {
            let rel_path = active
                .path
                .strip_prefix(&self.root)
                .unwrap_or(&active.path)
                .display();

            let cursor_info = match (active.cursor_line, active.cursor_col) {
                (Some(l), Some(c)) => format!(" (Cursor: Line {}, Col {})", l, c),
                (Some(l), None) => format!(" (Cursor: Line {})", l),
                _ => String::new(),
            };

            out.push_str(&format!("- Active Editor: `{}`{}\n", rel_path, cursor_info));

            if let Some(ref slice) = active.cursor_slice {
                out.push_str("\nActive Code Window:\n```\n");
                out.push_str(slice);
                out.push_str("```\n");
            }
        }

        let other_files: Vec<_> = self
            .open_files
            .iter()
            .filter(|f| !f.is_active)
            .map(|f| {
                f.path
                    .strip_prefix(&self.root)
                    .unwrap_or(&f.path)
                    .display()
                    .to_string()
            })
            .collect();

        if !other_files.is_empty() {
            out.push_str(&format!(
                "- Open Tabs: {}\n",
                other_files
                    .iter()
                    .map(|f| format!("`{}`", f))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_file_cursor_slice() {
        let content = "fn main() {\n    let a = 1;\n    let b = 2;\n    println!(\"{}\", a + b);\n}\n";
        let info = OpenFileInfo::new(PathBuf::from("/workspace/src/main.rs"), true)
            .with_cursor(3, 5, content, 1);

        assert_eq!(info.cursor_line, Some(3));
        assert_eq!(info.cursor_col, Some(5));
        let slice = info.cursor_slice.unwrap();
        assert!(slice.contains("2   "));
        assert!(slice.contains("let a = 1;"));
        assert!(slice.contains("3 >"));
        assert!(slice.contains("let b = 2;"));
        assert!(slice.contains("4   "));
        assert!(slice.contains("println!"));
    }

    #[test]
    fn test_workspace_context_prompt_formatting() {
        let ctx = WorkspaceContext::new(PathBuf::from("/my/project")).with_open_files(vec![
            OpenFileInfo::new(PathBuf::from("/my/project/src/lib.rs"), true),
            OpenFileInfo::new(PathBuf::from("/my/project/Cargo.toml"), false),
        ]);

        let prompt = ctx.format_for_prompt();
        assert!(prompt.contains("Workspace Root: `/my/project`"));
        assert!(prompt.contains("Active Editor: `src/lib.rs`"));
        assert!(prompt.contains("Open Tabs: `Cargo.toml`"));
    }
}
